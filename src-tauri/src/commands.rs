//! Tauri IPC commands invoked from the Svelte 5 frontend.
//!
//! Commands are async and push their work onto the blocking pool: device
//! enumeration, socket teardown, keyring and SQLite calls can each take
//! hundreds of milliseconds, and a sync command would freeze the window.
//! Errors are short codes (`mic_denied`, `profile_host_required`, ...) the UI
//! translates, or plain messages it shows as-is.

use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use streamy_core::audio::decoder::SUPPORTED_EXTENSIONS;
use streamy_core::audio::pipeline::LevelFeedFn;
use streamy_core::audio::player::{AddResult, PlayState, PlayerEvent, PlayerStatus, PlaylistPlayer, Track};
use streamy_core::audio::source::AudioSource;
use streamy_core::audio::switch::{InputControl, InputKind, SwitchSource};
use streamy_core::core::audio_config_rules::{AudioConfig, AudioConfigRules, BroadcastMode, Codec, DjProtocol};
use streamy_core::core::level::LevelTap;
use streamy_core::core::teardown_state::{RecordState, StreamState};
use streamy_core::session::record_session::RecordSession;
use streamy_core::session::stream_session::{StreamProfile, StreamSession, StreamStats};

use crate::audio_cpal::{AudioDeviceDesc, CpalAudioSource};
use crate::audio_out::MonitorOutput;
use crate::events::*;
use crate::power::PowerInhibitor;
use crate::secrets;
use crate::storage::db::{ProfileDb, ProfileRecord, KEYRING_SENTINEL};
use crate::storage::prefs::{AppSettings, SettingsStorage, MAX_GAIN, MIN_GAIN};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<ProfileDb>>,
    pub prefs: Arc<Mutex<SettingsStorage>>,
    pub power: Arc<PowerInhibitor>,
    pub stream_session: Arc<StreamSession>,
    pub record_session: Arc<RecordSession>,
    pub app_handle: Arc<Mutex<Option<AppHandle>>>,
    pub log_dir: PathBuf,
    pub mic_test: Arc<Mutex<Option<CpalAudioSource>>>,
    /// Which input is on air, and the mic gain; read live by the audio threads.
    pub input: Arc<InputControl>,
    pub player: Arc<PlaylistPlayer>,
    pub monitor: Arc<Mutex<Option<MonitorOutput>>>,
    pub playlist_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordingStatus {
    pub active: bool,
    /// True when the file is a copy of the live broadcast (no second mic capture).
    pub with_stream: bool,
    pub path: Option<String>,
    pub secs: u64,
    pub bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    pub stream: StreamState,
    pub stats: StreamStats,
    pub recording: RecordingStatus,
    pub mic_test: bool,
    pub input: InputKind,
    /// False when the running broadcast/recording could not open the mic.
    pub mic_available: bool,
    pub player: PlayerStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerSnapshot {
    pub status: PlayerStatus,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: String,
    pub lame_version: String,
    pub log_dir: String,
    pub recordings_dir: String,
}

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("internal task failed: {e}"))?
}

/// Resolve a stored secret: the sentinel means Credential Manager; any other
/// non-empty value is the plaintext fallback written when the keyring failed.
fn resolve_secret(stored: Option<&str>, keyring: impl FnOnce() -> Result<String, String>) -> Option<String> {
    match stored {
        Some(s) if s == KEYRING_SENTINEL => match keyring() {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!("credential lookup failed: {e}");
                None
            }
        },
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

impl AppState {
    fn handle(&self) -> Option<AppHandle> {
        self.app_handle.lock().clone()
    }

    fn level_feed(&self) -> Option<LevelFeedFn> {
        self.handle().map(throttled_level_feed)
    }

    pub fn recordings_dir(&self) -> PathBuf {
        let settings = self.prefs.lock().load();
        let dir = settings
            .recordings_folder
            .map(PathBuf::from)
            .unwrap_or_else(default_recordings_dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    pub fn status(&self) -> AppStatus {
        let stats = self.stream_session.stats();
        let record_state = self.record_session.state();
        let (rec_secs, rec_bytes) = self.record_session.progress();
        let recording = match record_state {
            RecordState::Recording { path } => RecordingStatus {
                active: true,
                with_stream: false,
                path: Some(path),
                secs: rec_secs,
                bytes: rec_bytes,
                error: None,
            },
            other if stats.recording => RecordingStatus {
                active: true,
                with_stream: true,
                path: stats.recording_path.clone(),
                secs: stats.recording_secs,
                bytes: stats.recording_bytes,
                error: match other {
                    RecordState::Error { reason } => Some(reason),
                    _ => None,
                },
            },
            RecordState::Error { reason } => RecordingStatus {
                active: false,
                with_stream: false,
                path: self.record_session.recording_path(),
                secs: 0,
                bytes: 0,
                error: Some(reason),
            },
            RecordState::Idle => RecordingStatus {
                active: false,
                with_stream: false,
                path: None,
                secs: 0,
                bytes: 0,
                error: None,
            },
        };
        AppStatus {
            stream: self.stream_session.state(),
            stats,
            recording,
            mic_test: self.mic_test.lock().is_some(),
            input: self.input.kind(),
            mic_available: !(self.capture_active() && self.input.mic_failed()),
            player: self.player.status(),
        }
    }

    fn capture_active(&self) -> bool {
        self.stream_session.is_active() || matches!(self.record_session.state(), RecordState::Recording { .. })
    }

    /// Mic + playlist behind the live input switch, at the encoder's format.
    fn switch_source(&self, device: Option<String>, sample_rate: u32, mono: bool) -> Box<dyn AudioSource> {
        Box::new(SwitchSource::new(
            Box::new(CpalAudioSource::new(device, sample_rate, mono, self.input.clone())),
            self.player.clone(),
            sample_rate,
            mono,
            self.input.clone(),
        ))
    }

    pub fn player_snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot {
            status: self.player.status(),
            tracks: self.player.tracks(),
        }
    }

    /// Restore the queue and player options saved last time.
    pub fn restore_player(&self, settings: &AppSettings) {
        self.player.set_volume(settings.player_volume);
        self.player.set_options(settings.auto_advance, settings.repeat);
        let paths: Vec<String> = std::fs::read_to_string(&self.playlist_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let existing: Vec<String> = paths.into_iter().filter(|p| std::path::Path::new(p).is_file()).collect();
        if !existing.is_empty() {
            let res = self.player.add_files(&existing);
            if !res.failed.is_empty() {
                tracing::warn!("{} saved playlist file(s) could not be read", res.failed.len());
            }
        }
    }

    fn save_playlist(&self) {
        let paths: Vec<String> = self.player.tracks().into_iter().map(|t| t.path).collect();
        let tmp = self.playlist_path.with_extension("tmp");
        let result = serde_json::to_string_pretty(&paths)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&tmp, json).map_err(|e| e.to_string()))
            .and_then(|_| std::fs::rename(&tmp, &self.playlist_path).map_err(|e| e.to_string()));
        if let Err(e) = result {
            tracing::warn!("saving playlist failed: {e}");
        }
    }

    /// Speakers follow the player while it has a track loaded and monitoring is on.
    fn sync_monitor(&self) {
        let want = self.prefs.lock().load().monitor_playlist && self.player.status().state != PlayState::Stopped;
        let mut monitor = self.monitor.lock();
        if want && monitor.is_none() {
            match MonitorOutput::start(self.player.clone()) {
                Ok(m) => *monitor = Some(m),
                Err(e) => tracing::warn!("playlist monitoring unavailable: {e}"),
            }
        } else if !want {
            if let Some(mut m) = monitor.take() {
                m.stop();
            }
        }
    }

    /// Tell listeners what's playing, if the playlist is what they hear.
    fn announce_track(&self, track: &Track) {
        if !self.stream_session.is_active() || self.input.kind() != InputKind::Playlist {
            return;
        }
        if !self.prefs.lock().load().send_track_titles {
            return;
        }
        if let Err(e) = self.stream_session.update_now_playing(&track.title) {
            tracing::warn!("track title update failed: {e}");
        }
    }

    /// Runs on the player's event worker thread, never the audio clock.
    pub fn on_player_event(&self, event: PlayerEvent) {
        match event {
            PlayerEvent::Changed => {
                self.save_playlist();
                self.sync_monitor();
            }
            PlayerEvent::TrackStarted(track) => self.announce_track(&track),
            PlayerEvent::TrackFailed { track, reason } => {
                if let Some(h) = self.handle() {
                    let _ = h.emit(EVENT_PLAYER_ERROR, PlayerErrorPayload { title: track.title, reason });
                }
            }
        }
        if let Some(h) = self.handle() {
            let _ = h.emit(EVENT_PLAYER_CHANGED, self.player_snapshot());
        }
    }

    pub fn set_input_source(&self, kind: InputKind) -> Result<(), String> {
        if kind == InputKind::Mic && self.capture_active() && self.input.mic_failed() {
            return Err("mic_unavailable".into());
        }
        self.input.set_kind(kind);
        let name = if kind == InputKind::Playlist { "playlist" } else { "mic" };
        self.prefs.lock().update(|s| s.input_source = name.into())?;
        if kind == InputKind::Playlist && self.player.status().state == PlayState::Playing {
            if let Some(t) = self.player.current_track() {
                self.announce_track(&t);
            }
        }
        Ok(())
    }

    pub fn stop_mic_test(&self) {
        let taken = self.mic_test.lock().take();
        if let Some(mut src) = taken {
            src.stop();
            if let Some(h) = self.handle() {
                emit_silence(&h);
            }
        }
    }

    pub fn start_mic_test(&self) -> Result<(), String> {
        self.stop_mic_test();
        let settings = self.prefs.lock().load();
        let mut src = CpalAudioSource::new(settings.input_device_name, 44_100, true, self.input.clone());
        let feed = self.level_feed();
        let input = self.input.clone();
        src.start(Box::new(move |pcm| {
            if let Some(ref feed) = feed {
                let g = input.mic_gain();
                let scaled: Vec<i16> = pcm
                    .iter()
                    .map(|&s| (s as f32 * g).clamp(-32768.0, 32767.0) as i16)
                    .collect();
                feed(streamy_core::core::level::LevelSnapshot {
                    samples: Vec::new(),
                    rms_db: LevelTap::rms_db(&scaled),
                    peak_db: LevelTap::peak_db(&scaled),
                });
            }
            true
        }))?;
        *self.mic_test.lock() = Some(src);
        Ok(())
    }

    fn arm_stream_recording(&self) -> Result<(), String> {
        if self.stream_session.is_recording() {
            return Ok(());
        }
        let settings = self.prefs.lock().load();
        let quality = AudioConfigRules::record_config(settings.record_bitrate_kbps, settings.record_mono);
        let path = self.recordings_dir().join(RecordSession::generate_filename());
        self.stream_session
            .arm_recording(path.to_string_lossy().to_string(), quality)?;
        if let Some(h) = self.handle() {
            let _ = h.emit(EVENT_STREAM_RECORDING, true);
        }
        Ok(())
    }

    /// Start broadcasting. Returns a non-fatal warning (e.g. the automatic
    /// recording could not be started) for the UI to show.
    pub fn start_stream(&self, profile_id: Option<i64>) -> Result<Option<String>, String> {
        let settings = self.prefs.lock().load();
        let id = profile_id
            .or(settings.selected_profile_id)
            .ok_or_else(|| "no_profile".to_string())?;
        let profile = self
            .db
            .lock()
            .get(id)
            .map_err(|e| format!("Database error: {e}"))?
            .ok_or_else(|| "profile_missing".to_string())?;

        let mode = BroadcastMode::parse(&profile.mode);
        let dj_protocol = DjProtocol::parse(&profile.dj_protocol);
        let password = resolve_secret(Some(&profile.password), || secrets::get_password(id));
        let dj_password = resolve_secret(profile.dj_password.as_deref(), || secrets::get_dj_password(id));

        let needs_password = match mode {
            BroadcastMode::SOURCE => password.is_none(),
            BroadcastMode::DJ => dj_password.is_none() && password.is_none(),
        };
        if needs_password {
            return Err("password_missing".into());
        }

        let stream_prof = StreamProfile {
            id: profile.id,
            name: profile.name,
            host: profile.host,
            port: profile.port,
            sid: profile.sid,
            password: password.unwrap_or_default(),
            mount: profile.mount,
            codec: Codec::MP3,
            bitrate_kbps: profile.bitrate_kbps,
            sample_rate_hz: profile.sample_rate_hz,
            mono: profile.mono,
            mode,
            dj_user: profile.dj_user,
            dj_password,
            dj_port: profile.dj_port,
            dj_protocol,
            dj_mount: profile.dj_mount,
            stream_name: profile.stream_name,
            genre: profile.genre,
            station_url: profile.station_url,
            public: profile.is_public,
            auto_reconnect: settings.auto_reconnect,
        };

        // The meter now follows the broadcast pipeline.
        self.stop_mic_test();

        let audio_source = self.switch_source(
            settings.input_device_name.clone(),
            stream_prof.sample_rate_hz,
            stream_prof.mono,
        );
        self.stream_session
            .start(stream_prof, audio_source, self.level_feed())?;

        if self.input.kind() == InputKind::Playlist && self.player.status().state == PlayState::Playing {
            if let Some(t) = self.player.current_track() {
                self.announce_track(&t);
            }
        }

        if settings.selected_profile_id != Some(id) {
            let _ = self.prefs.lock().update(|s| s.selected_profile_id = Some(id));
        }

        // Standalone recorder already running: don't record twice.
        let standalone = matches!(self.record_session.state(), RecordState::Recording { .. });
        if settings.record_with_stream && !standalone {
            if let Err(e) = self.arm_stream_recording() {
                tracing::warn!("auto-record failed: {e}");
                return Ok(Some(e));
            }
        }
        Ok(None)
    }

    pub fn stop_stream(&self) {
        let was_recording = self.stream_session.is_recording();
        self.stream_session.stop_and_shutdown(false);
        if let Some(h) = self.handle() {
            emit_silence(&h);
            if was_recording {
                let _ = h.emit(EVENT_STREAM_RECORDING, false);
            }
        }
    }

    /// One "Record" button: while on air, record the broadcast itself;
    /// otherwise open the mic for a standalone recording.
    pub fn start_recording(&self) -> Result<String, String> {
        if self.stream_session.is_active() {
            self.arm_stream_recording()?;
            return Ok(self.stream_session.recording_path().unwrap_or_default());
        }
        self.stop_mic_test();
        let settings = self.prefs.lock().load();
        let quality = AudioConfigRules::record_config(settings.record_bitrate_kbps, settings.record_mono);
        let path = self.recordings_dir().join(RecordSession::generate_filename());
        let source = self.switch_source(settings.input_device_name, quality.sample_rate_hz, quality.mono);
        self.record_session
            .start(quality, path, source, self.level_feed())
    }

    pub fn stop_recording(&self) {
        if self.stream_session.is_recording() {
            self.stream_session.disarm_recording();
            if let Some(h) = self.handle() {
                let _ = h.emit(EVENT_STREAM_RECORDING, false);
            }
        }
        if matches!(self.record_session.state(), RecordState::Recording { .. }) {
            self.record_session.stop_and_shutdown(false);
            if let Some(h) = self.handle() {
                emit_silence(&h);
            }
        }
    }

    pub fn set_gain(&self, gain: f32) -> f32 {
        let gain = if gain.is_finite() { gain.clamp(MIN_GAIN, MAX_GAIN) } else { 1.0 };
        // Applied by the input switch to mic audio only; the pipeline stays at unity.
        self.input.set_mic_gain(gain)
    }

    /// Anything the user would notice an update restart interrupting.
    pub fn is_busy(&self) -> bool {
        self.capture_active() || self.mic_test.lock().is_some() || self.player.status().state == PlayState::Playing
    }

    pub fn shutdown(&self) {
        self.stop_mic_test();
        self.stream_session.stop_and_shutdown(false);
        self.record_session.stop_and_shutdown(false);
        if let Some(mut m) = self.monitor.lock().take() {
            m.stop();
        }
        self.player.shutdown();
    }
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<AppStatus, String> {
    let st = state.inner().clone();
    blocking(move || Ok(st.status())).await
}

#[tauri::command]
pub async fn start_stream(state: State<'_, AppState>, profile_id: Option<i64>) -> Result<Option<String>, String> {
    let st = state.inner().clone();
    blocking(move || st.start_stream(profile_id)).await
}

#[tauri::command]
pub async fn stop_stream(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.stop_stream();
        Ok(())
    })
    .await
}

/// Returns `true` when the title went out live, `false` when queued for the next connect.
#[tauri::command]
pub async fn update_now_playing(state: State<'_, AppState>, title: String) -> Result<bool, String> {
    let st = state.inner().clone();
    blocking(move || st.stream_session.update_now_playing(&title)).await
}

#[tauri::command]
pub async fn set_mic_gain(state: State<'_, AppState>, gain: f32) -> Result<f32, String> {
    let st = state.inner().clone();
    blocking(move || {
        let gain = st.set_gain(gain);
        st.prefs.lock().update(|s| s.mic_gain = gain)?;
        Ok(gain)
    })
    .await
}

#[tauri::command]
pub async fn start_recording(state: State<'_, AppState>) -> Result<String, String> {
    let st = state.inner().clone();
    blocking(move || st.start_recording()).await
}

#[tauri::command]
pub async fn stop_recording(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.stop_recording();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn start_mic_test(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        if st.stream_session.is_active() || matches!(st.record_session.state(), RecordState::Recording { .. }) {
            return Ok(()); // the meter is already live
        }
        st.start_mic_test()
    })
    .await
}

#[tauri::command]
pub async fn stop_mic_test(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.stop_mic_test();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn list_input_devices() -> Result<Vec<AudioDeviceDesc>, String> {
    blocking(|| Ok(CpalAudioSource::list_devices())).await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let st = state.inner().clone();
    blocking(move || Ok(st.prefs.lock().load())).await
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<AppSettings, String> {
    let st = state.inner().clone();
    blocking(move || {
        let settings = settings.sanitized();
        let previous = st.prefs.lock().load();
        st.prefs.lock().save(&settings)?;
        st.set_gain(settings.mic_gain);
        st.input.set_noise_suppression(settings.noise_suppression);
        if previous.monitor_playlist != settings.monitor_playlist {
            st.sync_monitor();
        }

        // A different mic only applies to the next capture; restart the test meter now.
        if previous.input_device_name != settings.input_device_name && st.mic_test.lock().is_some() {
            let _ = st.start_mic_test();
        }

        if previous.launch_at_login != settings.launch_at_login {
            if let Some(h) = st.handle() {
                apply_autostart(&h, settings.launch_at_login)
                    .map_err(|e| format!("autostart_failed: {e}"))?;
            }
        }
        Ok(settings)
    })
    .await
}

pub fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let current = launcher.is_enabled().unwrap_or(!enabled);
    if current == enabled {
        return Ok(());
    }
    if enabled {
        launcher.enable().map_err(|e| e.to_string())
    } else {
        launcher.disable().map_err(|e| e.to_string())
    }
}

/// Never hand secrets to the webview; only whether they are set.
fn redact(mut p: ProfileRecord) -> ProfileRecord {
    p.has_password = !p.password.is_empty();
    p.has_dj_password = p.dj_password.as_deref().is_some_and(|s| !s.is_empty());
    p.password = String::new();
    p.dj_password = None;
    p.admin_password = None;
    p
}

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ProfileRecord>, String> {
    let st = state.inner().clone();
    blocking(move || {
        st.db
            .lock()
            .list()
            .map(|v| v.into_iter().map(redact).collect())
            .map_err(|e| format!("Failed to list profiles: {e}"))
    })
    .await
}

#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>, id: i64) -> Result<Option<ProfileRecord>, String> {
    let st = state.inner().clone();
    blocking(move || {
        st.db
            .lock()
            .get(id)
            .map(|p| p.map(redact))
            .map_err(|e| format!("Failed to get profile: {e}"))
    })
    .await
}

/// Users paste "http://host:8000/" into the host field; split it up.
fn split_host(raw: &str) -> (String, Option<u16>) {
    let mut h = raw.trim();
    for scheme in ["http://", "https://", "icecast://", "shoutcast://"] {
        if h.len() >= scheme.len() && h[..scheme.len()].eq_ignore_ascii_case(scheme) {
            h = &h[scheme.len()..];
        }
    }
    let h = h.split('/').next().unwrap_or("");
    // host:port (not IPv6, which has several colons)
    if h.matches(':').count() == 1 {
        let (host, port) = h.split_once(':').unwrap();
        if let Ok(p) = port.parse::<u16>() {
            return (host.to_string(), Some(p));
        }
    }
    (h.to_string(), None)
}

fn trimmed_opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Validate and normalize a profile from the UI. Errors are i18n codes.
pub fn validate_profile(mut p: ProfileRecord) -> Result<ProfileRecord, String> {
    let (host, port_from_host) = split_host(&p.host);
    p.host = host;
    if p.host.is_empty() {
        return Err("profile_host_required".into());
    }
    if let Some(port) = port_from_host {
        p.port = port;
    }
    if p.port == 0 {
        return Err("profile_port_invalid".into());
    }
    p.name = p.name.trim().to_string();
    if p.name.is_empty() {
        p.name = p.host.clone();
    }
    if p.sid == 0 {
        p.sid = 1;
    }

    p.codec = "MP3".into();
    if !AudioConfigRules::MP3_BITRATES.contains(&p.bitrate_kbps) {
        return Err("profile_bitrate_invalid".into());
    }
    if !AudioConfigRules::SAMPLE_RATES.contains(&p.sample_rate_hz) {
        return Err("profile_samplerate_invalid".into());
    }
    let clamped = AudioConfigRules::clamp(AudioConfig {
        codec: Codec::MP3,
        bitrate_kbps: p.bitrate_kbps,
        sample_rate_hz: p.sample_rate_hz,
        mono: p.mono,
    });
    p.sample_rate_hz = clamped.sample_rate_hz;

    let mode = BroadcastMode::parse(&p.mode);
    p.mode = format!("{mode:?}");
    let proto = DjProtocol::parse(&p.dj_protocol);
    p.dj_protocol = format!("{proto:?}");
    p.dj_user = trimmed_opt(p.dj_user);
    p.dj_mount = trimmed_opt(p.dj_mount);
    if mode == BroadcastMode::DJ && p.dj_port == Some(0) {
        return Err("profile_port_invalid".into());
    }
    if mode == BroadcastMode::DJ && proto == DjProtocol::ICECAST && p.dj_mount.is_none() {
        p.dj_mount = Some("/".into());
    }

    p.mount = p.mount.trim().to_string();
    p.stream_name = p.stream_name.trim().to_string();
    p.genre = p.genre.trim().to_string();
    p.station_url = p.station_url.trim().to_string();
    Ok(p)
}

#[tauri::command]
pub async fn save_profile(state: State<'_, AppState>, profile: ProfileRecord) -> Result<i64, String> {
    let st = state.inner().clone();
    blocking(move || save_profile_blocking(&st, profile)).await
}

fn save_profile_blocking(st: &AppState, profile: ProfileRecord) -> Result<i64, String> {
    let new_password = profile.password.clone();
    let new_dj_password = profile.dj_password.clone().unwrap_or_default();
    let mut record = validate_profile(profile)?;

    let existing = if record.id != 0 {
        Some(
            st.db
                .lock()
                .get(record.id)
                .map_err(|e| format!("Database error: {e}"))?
                .ok_or_else(|| "profile_missing".to_string())?,
        )
    } else {
        None
    };

    // Blank password fields mean "unchanged": the UI never receives the stored
    // secret, so it can only send a new one.
    record.password = existing.as_ref().map(|e| e.password.clone()).unwrap_or_default();
    record.dj_password = existing.as_ref().and_then(|e| e.dj_password.clone());
    record.admin_user = existing.as_ref().and_then(|e| e.admin_user.clone());
    record.admin_password = existing.as_ref().and_then(|e| e.admin_password.clone());

    let id = if let Some(ref e) = existing {
        record.id = e.id;
        st.db.lock().update(&record).map_err(|e| format!("Failed to update profile: {e}"))?;
        e.id
    } else {
        let id = st.db.lock().insert(&record).map_err(|e| format!("Failed to insert profile: {e}"))?;
        record.id = id;
        id
    };

    let mut changed = false;
    if !new_password.is_empty() && new_password != KEYRING_SENTINEL {
        record.password = match secrets::set_password(id, &new_password) {
            Ok(()) => KEYRING_SENTINEL.into(),
            Err(e) => {
                // Keep the profile usable; the plaintext stays in the local DB only.
                tracing::warn!("keyring write failed, storing password in the local database: {e}");
                new_password
            }
        };
        changed = true;
    }
    if !new_dj_password.is_empty() && new_dj_password != KEYRING_SENTINEL {
        record.dj_password = Some(match secrets::set_dj_password(id, &new_dj_password) {
            Ok(()) => KEYRING_SENTINEL.into(),
            Err(e) => {
                tracing::warn!("keyring write failed for DJ password, storing in the local database: {e}");
                new_dj_password
            }
        });
        changed = true;
    }
    if changed {
        st.db.lock().update(&record).map_err(|e| format!("Failed to update profile: {e}"))?;
    }

    // First profile becomes the selected one.
    let _ = st.prefs.lock().update(|s| {
        if s.selected_profile_id.is_none() {
            s.selected_profile_id = Some(id);
        }
    });
    Ok(id)
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        let _ = secrets::delete_password(id);
        st.db
            .lock()
            .delete(id)
            .map_err(|e| format!("Failed to delete profile: {e}"))?;
        let _ = st.prefs.lock().update(|s| {
            if s.selected_profile_id == Some(id) {
                s.selected_profile_id = None;
            }
        });
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn clear_all_profiles(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        let ids: Vec<i64> = st
            .db
            .lock()
            .list()
            .map(|v| v.into_iter().map(|p| p.id).collect())
            .unwrap_or_default();
        for id in ids {
            let _ = secrets::delete_password(id);
        }
        st.db
            .lock()
            .clear()
            .map_err(|e| format!("Failed to clear profiles: {e}"))?;
        let _ = st.prefs.lock().update(|s| s.selected_profile_id = None);
        Ok(())
    })
    .await
}

// ---- playlist ----------------------------------------------------------------

#[tauri::command]
pub async fn set_input_source(state: State<'_, AppState>, source: InputKind) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || st.set_input_source(source)).await
}

#[tauri::command]
pub async fn get_player(state: State<'_, AppState>) -> Result<PlayerSnapshot, String> {
    let st = state.inner().clone();
    blocking(move || Ok(st.player_snapshot())).await
}

/// Native multi-file picker; `None` when the user cancels.
#[tauri::command]
pub async fn playlist_pick_files(app: AppHandle, state: State<'_, AppState>) -> Result<Option<AddResult>, String> {
    use tauri_plugin_dialog::DialogExt;
    let st = state.inner().clone();
    blocking(move || {
        let picked = app
            .dialog()
            .file()
            .add_filter("Audio", SUPPORTED_EXTENSIONS)
            .blocking_pick_files();
        let Some(files) = picked else { return Ok(None) };
        let paths: Vec<String> = files
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        Ok(Some(st.player.add_files(&paths)))
    })
    .await
}

/// Add dropped files; folders and unsupported types are skipped.
#[tauri::command]
pub async fn playlist_add(state: State<'_, AppState>, paths: Vec<String>) -> Result<AddResult, String> {
    let st = state.inner().clone();
    blocking(move || {
        let audio: Vec<String> = paths
            .into_iter()
            .filter(|p| {
                let path = std::path::Path::new(p);
                path.is_file()
                    && path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            })
            .collect();
        Ok(st.player.add_files(&audio))
    })
    .await
}

#[tauri::command]
pub async fn playlist_remove(state: State<'_, AppState>, id: u64) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.player.remove(id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn playlist_move(state: State<'_, AppState>, id: u64, to: usize) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.player.move_track(id, to);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn playlist_clear(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.player.clear();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn player_play(state: State<'_, AppState>, id: Option<u64>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || st.player.play(id)).await
}

#[tauri::command]
pub async fn player_pause(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.player.pause();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn player_stop(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.player.stop();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn player_next(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || st.player.next()).await
}

#[tauri::command]
pub async fn player_previous(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || st.player.previous()).await
}

#[tauri::command]
pub async fn player_seek(state: State<'_, AppState>, secs: f64) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || st.player.seek(secs)).await
}

#[tauri::command]
pub async fn player_set_volume(state: State<'_, AppState>, volume: f32) -> Result<f32, String> {
    let st = state.inner().clone();
    blocking(move || {
        let v = st.player.set_volume(if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 1.0 });
        st.prefs.lock().update(|s| s.player_volume = v)?;
        Ok(v)
    })
    .await
}

#[tauri::command]
pub async fn player_set_options(state: State<'_, AppState>, auto_advance: bool, repeat: bool) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || {
        st.player.set_options(auto_advance, repeat);
        st.prefs.lock().update(|s| {
            s.auto_advance = auto_advance;
            s.repeat = repeat;
        })?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn get_lame_notice() -> String {
    include_str!("../../crates/streamy-lame/lame_notice.txt").to_string()
}

#[tauri::command]
pub fn get_update_status(tracker: State<'_, crate::updater::UpdateTracker>) -> crate::updater::UpdateStatus {
    tracker.status()
}

#[tauri::command]
pub async fn get_app_info(state: State<'_, AppState>) -> Result<AppInfo, String> {
    let st = state.inner().clone();
    blocking(move || {
        Ok(AppInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            lame_version: streamy_lame::lame_version().to_string(),
            log_dir: st.log_dir.to_string_lossy().to_string(),
            recordings_dir: st.recordings_dir().to_string_lossy().to_string(),
        })
    })
    .await
}

fn open_path(path: &std::path::Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(path);
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    std::process::Command::new(cmd)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot open folder: {e}"))
}

#[tauri::command]
pub async fn open_log_folder(state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.log_dir.clone();
    blocking(move || open_path(&dir)).await
}

#[tauri::command]
pub async fn open_recordings_folder(state: State<'_, AppState>) -> Result<(), String> {
    let st = state.inner().clone();
    blocking(move || open_path(&st.recordings_dir())).await
}

pub fn default_recordings_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile).join("Music").join("Streamy");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join("Music").join("Streamy");
    }
    PathBuf::from("recordings")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ProfileRecord {
        ProfileRecord {
            name: "Station".into(),
            host: "radio.example.com".into(),
            ..Default::default()
        }
    }

    #[test]
    fn pasted_url_is_split_into_host_and_port() {
        let p = validate_profile(ProfileRecord {
            host: " http://radio.example.com:8010/stream ".into(),
            ..base()
        })
        .unwrap();
        assert_eq!(p.host, "radio.example.com");
        assert_eq!(p.port, 8010);
    }

    #[test]
    fn blank_host_is_rejected_and_blank_name_defaults_to_host() {
        assert_eq!(
            validate_profile(ProfileRecord { host: "  ".into(), ..base() }).unwrap_err(),
            "profile_host_required"
        );
        let p = validate_profile(ProfileRecord { name: " ".into(), ..base() }).unwrap();
        assert_eq!(p.name, "radio.example.com");
    }

    #[test]
    fn codec_is_forced_to_mp3_and_low_bitrate_rate_clamped() {
        let p = validate_profile(ProfileRecord {
            codec: "AAC".into(),
            bitrate_kbps: 24,
            sample_rate_hz: 44_100,
            ..base()
        })
        .unwrap();
        assert_eq!(p.codec, "MP3");
        assert_eq!(p.sample_rate_hz, 22_050);
    }

    #[test]
    fn mode_and_protocol_are_canonicalized() {
        let p = validate_profile(ProfileRecord {
            mode: "dj".into(),
            dj_protocol: "icecast".into(),
            dj_user: Some("  ".into()),
            ..base()
        })
        .unwrap();
        assert_eq!(p.mode, "DJ");
        assert_eq!(p.dj_protocol, "ICECAST");
        assert_eq!(p.dj_user, None);
        assert_eq!(p.dj_mount.as_deref(), Some("/"));
    }

    #[test]
    fn redact_hides_secrets_but_reports_presence() {
        let p = redact(ProfileRecord {
            password: KEYRING_SENTINEL.into(),
            dj_password: Some("plain".into()),
            ..base()
        });
        assert!(p.has_password && p.has_dj_password);
        assert!(p.password.is_empty());
        assert!(p.dj_password.is_none());
    }

    #[test]
    fn resolve_secret_never_leaks_the_sentinel() {
        assert_eq!(resolve_secret(Some(KEYRING_SENTINEL), || Err("no entry".into())), None);
        assert_eq!(resolve_secret(Some(KEYRING_SENTINEL), || Ok("k".into())), Some("k".into()));
        assert_eq!(resolve_secret(Some("plain"), || Ok("k".into())), Some("plain".into()));
        assert_eq!(resolve_secret(Some(""), || Ok("k".into())), None);
    }
}

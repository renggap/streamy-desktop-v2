//! Tauri IPC commands invoked from the Svelte 5 frontend.

use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, State};

use streamy_core::audio::source::AudioSource;
use streamy_core::core::audio_config_rules::{AudioConfig, BroadcastMode, Codec, DjProtocol};
use streamy_core::core::teardown_state::{RecordState, StreamState};
use streamy_core::session::record_session::RecordSession;
use streamy_core::session::stream_session::{StreamProfile, StreamSession};

use crate::audio_cpal::{AudioDeviceDesc, CpalAudioSource};
use crate::events::*;
use crate::power::PowerInhibitor;
use crate::secrets;
use crate::storage::db::{ProfileDb, ProfileRecord};
use crate::storage::prefs::{AppSettings, SettingsStorage};

pub struct AppState {
    pub db: Arc<Mutex<ProfileDb>>,
    pub prefs: Arc<Mutex<SettingsStorage>>,
    pub power: Arc<PowerInhibitor>,
    pub stream_session: Arc<StreamSession>,
    pub record_session: Arc<RecordSession>,
    pub app_handle: Arc<parking_lot::Mutex<Option<AppHandle>>>,
    pub log_dir: PathBuf,
}

#[tauri::command]
pub fn get_stream_state(state: State<AppState>) -> StreamState {
    state.stream_session.state()
}

#[tauri::command]
pub fn start_stream(
    state: State<AppState>,
    profile_id: i64,
    device_name: Option<String>,
) -> Result<(), String> {
    let profile = {
        let db = state.db.lock();
        db.get(profile_id)
            .map_err(|e| format!("Database error: {:?}", e))?
            .ok_or_else(|| "profile_missing".to_string())?
    };

    // Load password: check Credential Manager first, fall back to record
    let password = secrets::get_password(profile_id).unwrap_or(profile.password);

    let stream_prof = StreamProfile {
        id: profile.id,
        name: profile.name,
        host: profile.host,
        port: profile.port,
        sid: profile.sid,
        password,
        mount: profile.mount,
        codec: Codec::parse(&profile.codec).unwrap_or(Codec::MP3),
        bitrate_kbps: profile.bitrate_kbps,
        sample_rate_hz: profile.sample_rate_hz,
        mono: profile.mono,
        mode: BroadcastMode::parse(&profile.mode),
        dj_user: profile.dj_user,
        dj_port: profile.dj_port,
        dj_protocol: DjProtocol::parse(&profile.dj_protocol),
        dj_mount: profile.dj_mount,
    };

    let audio_source: Box<dyn AudioSource> = Box::new(CpalAudioSource::new(
        device_name,
        stream_prof.sample_rate_hz,
        stream_prof.mono,
    ));

    let app_handle_opt = state.app_handle.lock().clone();
    let level_feed = app_handle_opt.map(|handle| {
        let feed: Arc<dyn Fn(streamy_core::core::level::LevelSnapshot) + Send + Sync> =
            Arc::new(move |snap| {
                let _ = handle.emit(EVENT_LEVELS, snap);
            });
        feed
    });

    let result = state.stream_session.start(stream_prof, audio_source, level_feed);
    // Acquire only on success: a failed start (mic denied, encoder init) must not
    // leave a permanent hold on the sleep inhibitor.
    if result.is_ok() {
        state.power.acquire();
    }
    result
}

#[tauri::command]
pub fn stop_stream(state: State<AppState>) {
    state.stream_session.stop_and_shutdown(false);
    state.power.release();
}

#[tauri::command]
pub fn update_now_playing(state: State<AppState>, title: String) -> Result<(), String> {
    state.stream_session.update_now_playing(&title)
}

#[tauri::command]
pub fn set_mic_gain(state: State<AppState>, gain: f32) -> Result<(), String> {
    state.stream_session.set_mic_gain(gain);
    state.record_session.set_mic_gain(gain);

    let prefs = state.prefs.lock();
    let mut s = prefs.load();
    s.mic_gain = gain;
    let _ = prefs.save(&s);
    Ok(())
}

#[tauri::command]
pub fn get_record_state(state: State<AppState>) -> RecordState {
    state.record_session.state()
}

#[tauri::command]
pub fn start_recording(
    state: State<AppState>,
    profile_id: Option<i64>,
    device_name: Option<String>,
) -> Result<String, String> {
    let settings = state.prefs.lock().load();
    let target_dir = settings
        .recordings_folder
        .map(PathBuf::from)
        .unwrap_or_else(|| default_recordings_dir());

    let filename = RecordSession::generate_filename();
    let file_path = target_dir.join(&filename);

    let (sample_rate_hz, bitrate_kbps, mono) = if let Some(id) = profile_id {
        let db = state.db.lock();
        if let Ok(Some(p)) = db.get(id) {
            (p.sample_rate_hz, p.bitrate_kbps, p.mono)
        } else {
            (44100, 128, true)
        }
    } else {
        (44100, 128, true)
    };

    let cfg = AudioConfig {
        codec: Codec::MP3,
        bitrate_kbps,
        sample_rate_hz,
        mono,
    };

    let audio_source: Box<dyn AudioSource> = Box::new(CpalAudioSource::new(
        device_name,
        cfg.sample_rate_hz,
        cfg.mono,
    ));

    let result = state.record_session.start(cfg, file_path, audio_source);
    // Same as start_stream: no hold on a failed start.
    if result.is_ok() {
        state.power.acquire();
    }
    result
}

#[tauri::command]
pub fn stop_recording(state: State<AppState>) {
    state.record_session.stop_and_shutdown(false);
    state.power.release();
}

#[tauri::command]
pub fn toggle_stream_recording(state: State<AppState>) -> Result<bool, String> {
    if state.stream_session.is_recording() {
        state.stream_session.disarm_recording();
        if let Some(ref handle) = *state.app_handle.lock() {
            let _ = handle.emit(EVENT_STREAM_RECORDING, false);
        }
        Ok(false)
    } else {
        let settings = state.prefs.lock().load();
        let target_dir = settings
            .recordings_folder
            .map(PathBuf::from)
            .unwrap_or_else(|| default_recordings_dir());

        let filename = RecordSession::generate_filename();
        let file_path = target_dir.join(&filename);

        state
            .stream_session
            .arm_recording(file_path.to_string_lossy().to_string())?;

        if let Some(ref handle) = *state.app_handle.lock() {
            let _ = handle.emit(EVENT_STREAM_RECORDING, true);
        }
        Ok(true)
    }
}

#[tauri::command]
pub fn list_input_devices() -> Vec<AudioDeviceDesc> {
    CpalAudioSource::list_devices()
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> AppSettings {
    state.prefs.lock().load()
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: AppSettings) -> Result<(), String> {
    state.prefs.lock().save(&settings)
}

#[tauri::command]
pub fn list_profiles(state: State<AppState>) -> Result<Vec<ProfileRecord>, String> {
    state
        .db
        .lock()
        .list()
        .map_err(|e| format!("Failed to list profiles: {:?}", e))
}

#[tauri::command]
pub fn get_profile(state: State<AppState>, id: i64) -> Result<Option<ProfileRecord>, String> {
    state
        .db
        .lock()
        .get(id)
        .map_err(|e| format!("Failed to get profile: {:?}", e))
}

#[tauri::command]
pub fn save_profile(state: State<AppState>, profile: ProfileRecord) -> Result<i64, String> {
    let mut db_record = profile.clone();
    let raw_password = profile.password;

    // Do not store cleartext password in SQLite if we can store it in Credential Manager
    db_record.password = "credential_manager".to_string();

    let id = if profile.id == 0 {
        state
            .db
            .lock()
            .insert(&db_record)
            .map_err(|e| format!("Failed to insert profile: {:?}", e))?
    } else {
        state
            .db
            .lock()
            .update(&db_record)
            .map_err(|e| format!("Failed to update profile: {:?}", e))?;
        profile.id
    };

    // Store in Windows Credential Manager. On keyring failure, keep the password
    // in the DB record so the profile still works, and surface a warning.
    if !raw_password.is_empty() && raw_password != "credential_manager" {
        if let Err(e) = secrets::set_password(id, &raw_password) {
            tracing::warn!("keyring write failed, falling back to DB storage: {}", e);
            let mut fallback = db_record.clone();
            fallback.id = id;
            fallback.password = raw_password.clone();
            let _ = state.db.lock().update(&fallback);
        }
    }

    Ok(id)
}

#[tauri::command]
pub fn delete_profile(state: State<AppState>, id: i64) -> Result<(), String> {
    let _ = secrets::delete_password(id);
    state
        .db
        .lock()
        .delete(id)
        .map_err(|e| format!("Failed to delete profile: {:?}", e))
}

#[tauri::command]
pub fn clear_all_profiles(state: State<AppState>) -> Result<(), String> {
    let ids = {
        let db = state.db.lock();
        db.list().map(|profiles| profiles.into_iter().map(|p| p.id).collect::<Vec<_>>())
    };
    if let Ok(ids) = ids {
        for id in ids {
            let _ = secrets::delete_password(id);
        }
    }
    state
        .db
        .lock()
        .clear()
        .map_err(|e| format!("Failed to clear profiles: {:?}", e))
}

#[tauri::command]
pub fn get_lame_notice() -> String {
    include_str!("../../crates/streamy-lame/lame_notice.txt").to_string()
}

#[tauri::command]
pub fn open_log_folder(state: State<AppState>) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(&state.log_dir)
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&state.log_dir)
            .spawn();
    }
    Ok(())
}

#[tauri::command]
pub fn open_recordings_folder(state: State<AppState>) -> Result<(), String> {
    let settings = state.prefs.lock().load();
    let folder = settings
        .recordings_folder
        .map(PathBuf::from)
        .unwrap_or_else(|| default_recordings_dir());

    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(&folder)
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&folder)
            .spawn();
    }
    Ok(())
}

fn default_recordings_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let path = PathBuf::from(profile).join("Music").join("Streamy");
            let _ = std::fs::create_dir_all(&path);
            return path;
        }
    }
    let p = PathBuf::from("recordings");
    let _ = std::fs::create_dir_all(&p);
    p
}

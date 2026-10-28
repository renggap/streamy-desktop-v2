//! Broadcast stream session management.
//! Manages lifecycle: Profile -> AudioPipeline -> StreamTransport -> SourceClient.
//! Implements drop baselining, 250ms Backpressure throttling, and idempotent teardown.
//!
//! Every start bumps a generation counter; callbacks from an older session
//! (late transport states, a mic error racing a user stop) carry their
//! generation and are ignored once it is stale.

use std::fs::File;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use serde::Serialize;

use crate::audio::pipeline::{AudioPipeline, LevelFeedFn};
use crate::audio::sink::Mp3DirectSink;
use crate::audio::source::AudioSource;
use crate::core::audio_config_rules::{AudioConfig, AudioConfigRules, BroadcastMode, Codec, DjProtocol};
use crate::core::backoff::MAX_ATTEMPTS;
use crate::core::teardown_state::{resolve_teardown_state, StreamState, TransportState};
use crate::net::client::{AbortHandle, SourceClient, StationInfo};
use crate::net::icecast::IcecastSourceClient;
use crate::net::transport::{ReconnectPolicy, StreamTransport};
use crate::net::uvox::UvoxClient;
use streamy_lame::Mp3Encoder;

pub const DROP_UI_THROTTLE_MS: u128 = 250;
/// A wrong host/port should surface quickly instead of walking the whole ladder.
pub const INITIAL_CONNECT_ATTEMPTS: u32 = 4;
/// How long stop waits for an in-flight send/handshake before forcing the socket shut.
const CLIENT_CLOSE_WAIT_MS: u64 = 300;
const DEFAULT_GENRE: &str = "Speech";

pub type StreamStateCallback = Arc<dyn Fn(StreamState) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct StreamProfile {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub sid: u32,
    pub password: String,
    pub mount: String,
    pub codec: Codec,
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
    pub mono: bool,
    pub mode: BroadcastMode,
    pub dj_user: Option<String>,
    pub dj_password: Option<String>,
    pub dj_port: Option<u16>,
    pub dj_protocol: DjProtocol,
    pub dj_mount: Option<String>,
    /// Station name announced to the server; falls back to `name` when blank.
    pub stream_name: String,
    pub genre: String,
    pub station_url: String,
    pub public: bool,
    pub auto_reconnect: bool,
}

impl Default for StreamProfile {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            host: String::new(),
            port: 8000,
            sid: 1,
            password: String::new(),
            mount: String::new(),
            codec: Codec::MP3,
            bitrate_kbps: 64,
            sample_rate_hz: 44_100,
            mono: true,
            mode: BroadcastMode::SOURCE,
            dj_user: None,
            dj_password: None,
            dj_port: None,
            dj_protocol: DjProtocol::ULTRAVOX,
            dj_mount: None,
            stream_name: String::new(),
            genre: String::new(),
            station_url: String::new(),
            public: true,
            auto_reconnect: true,
        }
    }
}

/// Live numbers for the UI, all measured in the core (not guessed by the UI).
#[derive(Debug, Clone, Default, Serialize)]
pub struct StreamStats {
    pub bytes_sent: u64,
    /// Seconds since the current connection went live; 0 when not live.
    pub live_secs: u64,
    /// Seconds since start() was called; 0 when idle.
    pub session_secs: u64,
    pub dropped_frames: u64,
    pub recording: bool,
    pub recording_path: Option<String>,
    pub recording_bytes: u64,
    pub recording_secs: u64,
    pub now_playing: Option<String>,
}

type SharedClient = Arc<Mutex<Box<dyn SourceClient>>>;

struct Active {
    pipeline: AudioPipeline,
    transport: Arc<StreamTransport>,
    client: SharedClient,
    abort: AbortHandle,
    started: Instant,
}

/// Title to (re)announce: set by the UI, delivered by the transport worker
/// right after a connect and between audio frames.
#[derive(Default)]
struct NowPlaying {
    title: Mutex<Option<String>>,
    pending: AtomicBool,
}

pub struct StreamSession {
    active: Mutex<Option<Active>>,
    generation: AtomicU64,
    current_state: Mutex<StreamState>,
    state_callback: StreamStateCallback,

    live_drop_baseline: AtomicU64,
    last_drop_ui_emit: Mutex<Instant>,
    live_since: Mutex<Option<Instant>>,
    mic_gain: Mutex<f32>,
    now_playing: Arc<NowPlaying>,

    is_recording: AtomicBool,
    recording_path: Mutex<Option<String>>,
    recording_since: Mutex<Option<Instant>>,
}

impl StreamSession {
    pub fn new(state_callback: StreamStateCallback) -> Arc<Self> {
        Arc::new(Self {
            active: Mutex::new(None),
            generation: AtomicU64::new(0),
            current_state: Mutex::new(StreamState::Idle),
            state_callback,
            live_drop_baseline: AtomicU64::new(0),
            last_drop_ui_emit: Mutex::new(Instant::now()),
            live_since: Mutex::new(None),
            mic_gain: Mutex::new(1.0),
            now_playing: Arc::new(NowPlaying::default()),
            is_recording: AtomicBool::new(false),
            recording_path: Mutex::new(None),
            recording_since: Mutex::new(None),
        })
    }

    pub fn state(&self) -> StreamState {
        self.current_state.lock().clone()
    }

    pub fn is_active(&self) -> bool {
        self.active.lock().is_some()
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn recording_path(&self) -> Option<String> {
        self.recording_path.lock().clone()
    }

    pub fn set_mic_gain(&self, gain: f32) {
        *self.mic_gain.lock() = gain;
        if let Some(ref a) = *self.active.lock() {
            a.pipeline.set_gain(gain);
        }
    }

    pub fn stats(&self) -> StreamStats {
        let active = self.active.lock();
        let Some(a) = active.as_ref() else {
            return StreamStats {
                now_playing: self.now_playing.title.lock().clone(),
                ..Default::default()
            };
        };
        let recording_path = self.recording_path.lock().clone();
        let recording = self.is_recording();
        let recording_bytes = match (&recording_path, recording) {
            (Some(p), true) => std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
            _ => 0,
        };
        let baseline = self.live_drop_baseline.load(Ordering::SeqCst);
        StreamStats {
            bytes_sent: a.transport.bytes_sent(),
            live_secs: self.live_since.lock().map(|t| t.elapsed().as_secs()).unwrap_or(0),
            session_secs: a.started.elapsed().as_secs(),
            dropped_frames: a.transport.dropped_frames().saturating_sub(baseline),
            recording,
            recording_path: if recording { recording_path } else { None },
            recording_bytes,
            recording_secs: if recording {
                self.recording_since.lock().map(|t| t.elapsed().as_secs()).unwrap_or(0)
            } else {
                0
            },
            now_playing: self.now_playing.title.lock().clone(),
        }
    }

    pub fn start(
        self: &Arc<Self>,
        profile: StreamProfile,
        audio_source: Box<dyn AudioSource>,
        level_feed: Option<LevelFeedFn>,
    ) -> Result<(), String> {
        self.stop_and_shutdown(false);

        if profile.host.trim().is_empty() {
            return Err("No server address configured for this profile".into());
        }

        // Only a LAME encoder ships, so the wire format is always MP3. Sending
        // MP3 frames labelled audio/aacp makes DNAS reject or garble the stream.
        if profile.codec != Codec::MP3 {
            tracing::warn!("profile codec {:?} unsupported by this build; streaming MP3", profile.codec);
        }
        let cfg = AudioConfigRules::clamp(AudioConfig {
            codec: Codec::MP3,
            bitrate_kbps: profile.bitrate_kbps,
            sample_rate_hz: profile.sample_rate_hz,
            mono: profile.mono,
        });

        let encoder = Mp3Encoder::new(cfg.sample_rate_hz, cfg.bitrate_kbps, cfg.mono)
            .map_err(|e| format!("Failed to create MP3 encoder: {:?}", e))?;

        let station = StationInfo {
            url: if profile.station_url.trim().is_empty() {
                StationInfo::default().url
            } else {
                profile.station_url.trim().to_string()
            },
            public: profile.public,
        };
        let channels = if cfg.mono { 1 } else { 2 };
        let mime = "audio/mpeg".to_string();

        let is_dj = profile.mode == BroadcastMode::DJ;
        let source: Box<dyn SourceClient> = if !is_dj {
            Box::new(
                UvoxClient::new(
                    profile.host.clone(),
                    profile.port,
                    profile.sid,
                    profile.password.clone(),
                    mime,
                    cfg.sample_rate_hz,
                    channels,
                    None,
                )
                .with_station(station),
            )
        } else {
            // DJ mode: authenticate with the DJ credential pair
            // (dj_user + dj_password), not the source password.
            let dj_port = profile.dj_port.unwrap_or(profile.port);
            let dj_password = profile
                .dj_password
                .clone()
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| profile.password.clone());
            match profile.dj_protocol {
                DjProtocol::ULTRAVOX => Box::new(
                    UvoxClient::new(
                        profile.host.clone(),
                        dj_port,
                        profile.sid,
                        dj_password,
                        mime,
                        cfg.sample_rate_hz,
                        channels,
                        profile.dj_user.clone().filter(|u| !u.trim().is_empty()),
                    )
                    .with_station(station),
                ),
                DjProtocol::ICECAST => Box::new(
                    IcecastSourceClient::new(
                        profile.host.clone(),
                        dj_port,
                        profile.dj_mount.clone().unwrap_or_else(|| "/".into()),
                        profile.dj_user.clone().unwrap_or_default(),
                        dj_password,
                        mime,
                    )
                    .with_station(station),
                ),
            }
        };

        let abort = source.abort_handle();
        let client: SharedClient = Arc::new(Mutex::new(source));

        // Claim a generation for this session; everything below is tagged with it.
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let station_name = if profile.stream_name.trim().is_empty() {
            profile.name.trim().to_string()
        } else {
            profile.stream_name.trim().to_string()
        };
        let genre = if profile.genre.trim().is_empty() {
            DEFAULT_GENRE.to_string()
        } else {
            profile.genre.trim().to_string()
        };

        // Connect callback (transport worker, blocking pool)
        let client_conn = client.clone();
        let np_conn = self.now_playing.clone();
        let bitrate_kbps = cfg.bitrate_kbps;
        let connect_fn = Arc::new(move || {
            let mut cl = client_conn.lock();
            cl.connect(bitrate_kbps, &station_name, &genre)?;
            // A fresh connection has no title yet: re-announce the current one.
            if np_conn.title.lock().is_some() {
                np_conn.pending.store(true, Ordering::SeqCst);
            }
            Ok(())
        });

        // Send callback (transport worker). Pending titles ride between frames so
        // the UI never blocks on the client lock.
        let client_send = client.clone();
        let np_send = self.now_playing.clone();
        let send_fn = Arc::new(move |frame: &[u8]| {
            let mut cl = client_send.lock();
            cl.send(frame)?;
            if np_send.pending.swap(false, Ordering::SeqCst) {
                let title = np_send.title.lock().clone();
                if let Some(t) = title {
                    if let Err(e) = cl.update_now_playing(&t) {
                        tracing::warn!("now-playing update failed: {e}");
                    }
                }
            }
            Ok(())
        });

        let session_weak = Arc::downgrade(self);
        let on_state_fn = Arc::new(move |ts: TransportState| {
            if let Some(session) = session_weak.upgrade() {
                session.handle_transport_state(gen, ts);
            }
        });

        let transport = Arc::new(StreamTransport::new(connect_fn, send_fn, on_state_fn, None));
        transport.set_policy(ReconnectPolicy {
            auto_reconnect: profile.auto_reconnect,
            initial_max_attempts: INITIAL_CONNECT_ATTEMPTS,
            max_attempts: MAX_ATTEMPTS,
        });
        // Whatever ends the worker (stop, give-up, failure), the socket closes there.
        let client_exit = client.clone();
        transport.set_on_exit(Box::new(move || {
            client_exit.lock().close();
        }));

        let session_for_mic = Arc::downgrade(self);
        let on_mic_error = Arc::new(move |reason: String| {
            if let Some(session) = session_for_mic.upgrade() {
                session.fail(gen, reason);
            }
        });

        let mut pipeline = AudioPipeline::new(
            audio_source,
            encoder,
            Some(transport.clone()),
            None,
            *self.mic_gain.lock(),
            on_mic_error,
            level_feed,
        );

        if let Err(e) = pipeline.start() {
            pipeline.stop();
            transport.stop();
            client.lock().close();
            return Err(e);
        }

        {
            let mut active = self.active.lock();
            *active = Some(Active {
                pipeline,
                transport: transport.clone(),
                client,
                abort,
                started: Instant::now(),
            });
        }
        self.live_drop_baseline.store(0, Ordering::SeqCst);
        *self.live_since.lock() = None;

        self.set_state(StreamState::Connecting);
        transport.start();

        Ok(())
    }

    fn is_current(&self, gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == gen
    }

    fn handle_transport_state(self: &Arc<Self>, gen: u64, ts: TransportState) {
        if !self.is_current(gen) {
            return;
        }
        match ts {
            TransportState::Connecting => {
                *self.live_since.lock() = None;
                self.set_state(StreamState::Connecting);
            }
            TransportState::Live => {
                let baseline = self
                    .active
                    .lock()
                    .as_ref()
                    .map(|a| a.transport.dropped_frames())
                    .unwrap_or(0);
                self.live_drop_baseline.store(baseline, Ordering::SeqCst);
                *self.live_since.lock() = Some(Instant::now());
                self.set_state(StreamState::Live { dropped_frames: 0 });
            }
            TransportState::Reconnecting { attempt } => {
                *self.live_since.lock() = None;
                self.set_state(StreamState::Reconnecting { attempt });
            }
            TransportState::Backoff { attempt, .. } => {
                *self.live_since.lock() = None;
                let state = if attempt == 0 {
                    StreamState::Connecting
                } else {
                    StreamState::Reconnecting { attempt }
                };
                self.set_state(state);
            }
            TransportState::Backpressure { drops } => {
                let baseline = self.live_drop_baseline.load(Ordering::SeqCst);
                let current_drops = drops.saturating_sub(baseline);

                let mut last_emit = self.last_drop_ui_emit.lock();
                if last_emit.elapsed().as_millis() >= DROP_UI_THROTTLE_MS {
                    *last_emit = Instant::now();
                    drop(last_emit);
                    let current = self.current_state.lock().clone();
                    if let StreamState::Live { dropped_frames } = current {
                        if dropped_frames != current_drops {
                            self.set_state(StreamState::Live {
                                dropped_frames: current_drops,
                            });
                        }
                    }
                }
            }
            TransportState::Failed { reason } => {
                self.fail(gen, reason);
            }
        }
    }

    /// Terminal failure of session `gen`: surface the error, then release the
    /// mic, encoder and socket. Teardown runs on its own thread because this is
    /// called from the transport worker or a capture callback, which teardown joins.
    fn fail(self: &Arc<Self>, gen: u64, reason: String) {
        if !self.is_current(gen) {
            return;
        }
        tracing::warn!("stream session failed: {reason}");
        self.set_state(StreamState::Error { reason });
        let me = self.clone();
        std::thread::spawn(move || {
            if let Some(active) = me.take_active(Some(gen)) {
                me.teardown(active);
                let terminal = resolve_teardown_state(&me.current_state.lock(), true);
                me.set_state_if_changed(terminal);
            }
        });
    }

    fn set_state(&self, new_state: StreamState) {
        *self.current_state.lock() = new_state.clone();
        (self.state_callback)(new_state);
    }

    fn set_state_if_changed(&self, new_state: StreamState) {
        let changed = {
            let mut cur = self.current_state.lock();
            if *cur == new_state {
                false
            } else {
                *cur = new_state.clone();
                true
            }
        };
        if changed {
            (self.state_callback)(new_state);
        }
    }

    /// Set the title shown to listeners. Returns `true` when it goes out right
    /// away (live), `false` when it is queued and sent once the stream connects.
    pub fn update_now_playing(&self, title: &str) -> Result<bool, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Title is empty".into());
        }
        *self.now_playing.title.lock() = Some(title.to_string());
        let live = matches!(self.state(), StreamState::Live { .. });
        if live {
            self.now_playing.pending.store(true, Ordering::SeqCst);
        }
        Ok(live)
    }

    pub fn now_playing(&self) -> Option<String> {
        self.now_playing.title.lock().clone()
    }

    pub fn arm_recording(&self, path: String) -> Result<(), String> {
        let active = self.active.lock();
        let Some(a) = active.as_ref() else {
            // Checked before touching the disk so a refused arm leaves no empty file.
            return Err("Start the broadcast before recording it".into());
        };
        // Refuse to clobber an existing recording (same guard as RecordSession).
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("Cannot create recording file {path:?}: {e}"))?;
        a.pipeline.arm_record(Box::new(Mp3DirectSink::new(file)));
        *self.recording_path.lock() = Some(path);
        *self.recording_since.lock() = Some(Instant::now());
        self.is_recording.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn disarm_recording(&self) {
        if let Some(ref a) = *self.active.lock() {
            a.pipeline.disarm_record();
        }
        self.is_recording.store(false, Ordering::SeqCst);
        *self.recording_since.lock() = None;
    }

    /// Take the active session, retiring its generation. With `only_gen`, only
    /// if that generation is still the current one.
    fn take_active(&self, only_gen: Option<u64>) -> Option<Active> {
        let mut guard = self.active.lock();
        if let Some(g) = only_gen {
            if !self.is_current(g) {
                return None;
            }
        }
        let taken = guard.take();
        if taken.is_some() {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        taken
    }

    fn teardown(&self, mut active: Active) {
        self.is_recording.store(false, Ordering::SeqCst);
        *self.recording_since.lock() = None;
        *self.live_since.lock() = None;

        // Capture first: the encoder flush enqueues the trailing frame(s).
        active.pipeline.disarm_record();
        active.pipeline.stop();
        active.transport.stop();

        // The worker closes the client on exit. If it's mid-handshake or stuck in
        // a write, don't wait on its lock: shut the socket to unblock it.
        match active.client.try_lock_for(Duration::from_millis(CLIENT_CLOSE_WAIT_MS)) {
            Some(mut cl) => cl.close(),
            None => active.abort.abort(),
        }
    }

    /// Idempotent stop. `preserve_terminal` keeps an Error state visible.
    pub fn stop_and_shutdown(&self, preserve_terminal: bool) {
        // Retire the generation even when nothing is active, so callbacks that
        // were already in flight can't resurrect state.
        let active = self.take_active(None);
        if let Some(a) = active {
            self.teardown(a);
        }
        let terminal = resolve_teardown_state(&self.current_state.lock(), preserve_terminal);
        self.set_state_if_changed(terminal);
    }
}

impl Drop for StreamSession {
    fn drop(&mut self) {
        if let Some(a) = self.active.lock().take() {
            self.teardown(a);
        }
    }
}

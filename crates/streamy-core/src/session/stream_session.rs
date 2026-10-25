//! Broadcast stream session management.
//! Manages lifecycle: Profile -> AudioPipeline -> StreamTransport -> SourceClient.
//! Implements drop baselining, 250ms Backpressure throttling, and idempotent teardown.

use std::fs::File;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use parking_lot::Mutex;

use crate::audio::pipeline::{AudioPipeline, LevelFeedFn};
use crate::audio::sink::Mp3DirectSink;
use crate::audio::source::AudioSource;
use crate::core::audio_config_rules::{AudioConfig, AudioConfigRules, BroadcastMode, Codec, DjProtocol};
use crate::core::teardown_state::{resolve_teardown_state, StreamState, TransportState};
use crate::net::client::SourceClient;
use crate::net::icecast::IcecastSourceClient;
use crate::net::transport::StreamTransport;
use crate::net::uvox::UvoxClient;
use streamy_lame::Mp3Encoder;

pub const DROP_UI_THROTTLE_MS: u128 = 250;

pub type StreamStateCallback = Arc<dyn Fn(StreamState) + Send + Sync>;

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
    pub dj_port: Option<u16>,
    pub dj_protocol: DjProtocol,
    pub dj_mount: Option<String>,
}

pub struct StreamSession {
    pipeline: Mutex<Option<AudioPipeline>>,
    transport: Mutex<Option<Arc<StreamTransport>>>,
    client: Mutex<Option<Arc<parking_lot::Mutex<Box<dyn SourceClient>>>>>,
    current_state: Mutex<StreamState>,
    state_callback: StreamStateCallback,

    live_drop_baseline: AtomicU64,
    last_drop_ui_emit: Mutex<Instant>,
    shutting_down: AtomicBool,
    mic_gain: Mutex<f32>,

    is_recording: AtomicBool,
    recording_path: Mutex<Option<String>>,
}

impl StreamSession {
    pub fn new(state_callback: StreamStateCallback) -> Arc<Self> {
        Arc::new(Self {
            pipeline: Mutex::new(None),
            transport: Mutex::new(None),
            client: Mutex::new(None),
            current_state: Mutex::new(StreamState::Idle),
            state_callback,
            live_drop_baseline: AtomicU64::new(0),
            last_drop_ui_emit: Mutex::new(Instant::now()),
            shutting_down: AtomicBool::new(false),
            mic_gain: Mutex::new(1.0),
            is_recording: AtomicBool::new(false),
            recording_path: Mutex::new(None),
        })
    }

    pub fn state(&self) -> StreamState {
        self.current_state.lock().clone()
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn recording_path(&self) -> Option<String> {
        self.recording_path.lock().clone()
    }

    pub fn set_mic_gain(&self, gain: f32) {
        *self.mic_gain.lock() = gain;
        if let Some(ref p) = *self.pipeline.lock() {
            p.set_gain(gain);
        }
    }

    pub fn start(
        self: &Arc<Self>,
        profile: StreamProfile,
        audio_source: Box<dyn AudioSource>,
        level_feed: Option<LevelFeedFn>,
    ) -> Result<(), String> {
        self.stop_and_shutdown(false);
        self.shutting_down.store(false, Ordering::SeqCst);

        // Clamp impossible combinations
        let raw_cfg = AudioConfig {
            codec: profile.codec,
            bitrate_kbps: profile.bitrate_kbps,
            sample_rate_hz: profile.sample_rate_hz,
            mono: profile.mono,
        };
        let cfg = AudioConfigRules::clamp(raw_cfg);

        // Create LAME encoder
        let encoder = Mp3Encoder::new(cfg.sample_rate_hz, cfg.bitrate_kbps, cfg.mono)
            .map_err(|e| format!("Failed to create MP3 encoder: {:?}", e))?;

        // Resolve SourceClient
        let is_dj = profile.mode == BroadcastMode::DJ;
        let dj_port = if is_dj {
            profile.dj_port.unwrap_or(profile.port)
        } else {
            profile.port
        };

        let source: Box<dyn SourceClient> = if !is_dj {
            Box::new(UvoxClient::new(
                profile.host.clone(),
                profile.port,
                profile.sid,
                profile.password.clone(),
                if cfg.codec == Codec::AAC { "audio/aacp".into() } else { "audio/mpeg".into() },
                cfg.sample_rate_hz,
                if cfg.mono { 1 } else { 2 },
                None,
            ))
        } else {
            match profile.dj_protocol {
                DjProtocol::ULTRAVOX => Box::new(UvoxClient::new(
                    profile.host.clone(),
                    dj_port,
                    profile.sid,
                    profile.password.clone(),
                    if cfg.codec == Codec::AAC { "audio/aacp".into() } else { "audio/mpeg".into() },
                    cfg.sample_rate_hz,
                    if cfg.mono { 1 } else { 2 },
                    profile.dj_user.clone(),
                )),
                DjProtocol::ICECAST => Box::new(IcecastSourceClient::new(
                    profile.host.clone(),
                    dj_port,
                    profile.dj_mount.clone().unwrap_or_else(|| "/".into()),
                    profile.dj_user.clone().unwrap_or_default(),
                    profile.password.clone(),
                    "audio/mpeg".into(),
                )),
            }
        };

        let client_arc = Arc::new(parking_lot::Mutex::new(source));
        *self.client.lock() = Some(client_arc.clone());

        // Connect callback
        let client_conn = client_arc.clone();
        let profile_name = profile.name.clone();
        let bitrate_kbps = cfg.bitrate_kbps;
        let connect_fn = Arc::new(move || {
            let mut cl = client_conn.lock();
            cl.connect(bitrate_kbps, &profile_name, "Speech")
        });

        // Send callback
        let client_send = client_arc.clone();
        let send_fn = Arc::new(move |frame: &[u8]| {
            let mut cl = client_send.lock();
            cl.send(frame)
        });

        // State callback mapping
        let session_weak = Arc::downgrade(self);
        let on_state_fn = Arc::new(move |ts: TransportState| {
            if let Some(session) = session_weak.upgrade() {
                if session.shutting_down.load(Ordering::SeqCst) {
                    return;
                }
                session.handle_transport_state(ts);
            }
        });

        let transport = Arc::new(StreamTransport::new(
            connect_fn,
            send_fn,
            on_state_fn,
            None,
        ));
        *self.transport.lock() = Some(transport.clone());

        // Mic busy handler
        let session_for_mic = Arc::downgrade(self);
        let on_mic_busy = Arc::new(move || {
            if let Some(session) = session_for_mic.upgrade() {
                session.set_state(StreamState::Error {
                    reason: "mic_busy".into(),
                });
                session.stop_and_shutdown(true);
            }
        });

        let mut pipeline = AudioPipeline::new(
            audio_source,
            encoder,
            Some(transport.clone()),
            None,
            *self.mic_gain.lock(),
            on_mic_busy,
            level_feed,
        );

        pipeline.start()?;
        *self.pipeline.lock() = Some(pipeline);

        self.set_state(StreamState::Connecting);
        transport.start();

        Ok(())
    }

    fn handle_transport_state(&self, ts: TransportState) {
        match ts {
            TransportState::Connecting => {
                self.set_state(StreamState::Connecting);
            }
            TransportState::Live => {
                let baseline = if let Some(ref t) = *self.transport.lock() {
                    t.dropped_frames()
                } else {
                    0
                };
                self.live_drop_baseline.store(baseline, Ordering::SeqCst);
                self.set_state(StreamState::Live { dropped_frames: 0 });
            }
            TransportState::Reconnecting { attempt } => {
                self.set_state(StreamState::Reconnecting { attempt });
            }
            TransportState::Backoff { attempt, .. } => {
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
                    let current = self.current_state.lock().clone();
                    if let StreamState::Live { dropped_frames } = current {
                        if dropped_frames != current_drops {
                            drop(last_emit);
                            self.set_state(StreamState::Live {
                                dropped_frames: current_drops,
                            });
                        }
                    }
                }
            }
            TransportState::Failed { reason } => {
                self.set_state(StreamState::Error { reason });
            }
        }
    }

    fn set_state(&self, new_state: StreamState) {
        *self.current_state.lock() = new_state.clone();
        (self.state_callback)(new_state);
    }

    pub fn update_now_playing(&self, title: &str) -> Result<(), String> {
        if let Some(ref client_mutex) = *self.client.lock() {
            let mut cl = client_mutex.lock();
            cl.update_now_playing(title)
                .map_err(|e| format!("Failed to update now playing: {:?}", e))?;
        }
        Ok(())
    }

    pub fn arm_recording(&self, path: String) -> Result<(), String> {
        let file = File::create(&path).map_err(|e| format!("Failed to create recording file: {:?}", e))?;
        let sink = Box::new(Mp3DirectSink::new(file));

        if let Some(ref p) = *self.pipeline.lock() {
            p.arm_record(sink);
            *self.recording_path.lock() = Some(path);
            self.is_recording.store(true, Ordering::SeqCst);
            Ok(())
        } else {
            Err("Pipeline is not running".into())
        }
    }

    pub fn disarm_recording(&self) {
        if let Some(ref p) = *self.pipeline.lock() {
            p.disarm_record();
        }
        self.is_recording.store(false, Ordering::SeqCst);
    }

    pub fn stop_and_shutdown(&self, preserve_terminal: bool) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }

        self.disarm_recording();

        if let Some(mut p) = self.pipeline.lock().take() {
            p.stop();
        }

        if let Some(t) = self.transport.lock().take() {
            t.stop();
        }

        if let Some(c) = self.client.lock().take() {
            c.lock().close();
        }

        let terminal = resolve_teardown_state(&self.current_state.lock(), preserve_terminal);
        self.set_state(terminal);
    }
}

//! Standalone MP3 recording session.
//! Mic -> LAME -> local MP3 file. Runs independently of any live broadcast.

use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;

use crate::audio::pipeline::AudioPipeline;
use crate::audio::sink::{Mp3DirectSink, StreamSink};
use crate::audio::source::AudioSource;
use crate::core::audio_config_rules::{AudioConfig, AudioConfigRules};
use crate::core::teardown_state::{resolve_record_teardown_state, RecordState};
use streamy_lame::Mp3Encoder;

pub type RecordStateCallback = Arc<dyn Fn(RecordState) + Send + Sync>;

pub struct RecordSession {
    pipeline: Mutex<Option<AudioPipeline>>,
    current_state: Mutex<RecordState>,
    state_callback: RecordStateCallback,
    shutting_down: AtomicBool,
    recording_path: Mutex<Option<String>>,
    mic_gain: Mutex<f32>,
}

impl RecordSession {
    pub fn new(state_callback: RecordStateCallback) -> Arc<Self> {
        Arc::new(Self {
            pipeline: Mutex::new(None),
            current_state: Mutex::new(RecordState::Idle),
            state_callback,
            shutting_down: AtomicBool::new(false),
            recording_path: Mutex::new(None),
            mic_gain: Mutex::new(1.0),
        })
    }

    pub fn state(&self) -> RecordState {
        self.current_state.lock().clone()
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

    /// Generate standard filename: `streamy-<yyyyMMdd-HHmmss>-<ms>.mp3`.
    pub fn generate_filename() -> String {
        let now = chrono_or_system_time();
        format!("streamy-{}.mp3", now)
    }

    pub fn start(
        self: &Arc<Self>,
        config: AudioConfig,
        output_path: PathBuf,
        audio_source: Box<dyn AudioSource>,
    ) -> Result<String, String> {
        self.stop_and_shutdown(false);
        self.shutting_down.store(false, Ordering::SeqCst);

        let cfg = AudioConfigRules::clamp(config);

        // Ensure parent directory exists
        if let Some(parent) = output_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file = File::create(&output_path)
            .map_err(|e| format!("Failed to create recording file: {:?}", e))?;
        let display_path = output_path.to_string_lossy().to_string();

        let encoder = Mp3Encoder::new(cfg.sample_rate_hz, cfg.bitrate_kbps, cfg.mono)
            .map_err(|e| format!("Failed to create LAME encoder: {:?}", e))?;

        let sink: Box<dyn StreamSink> = Box::new(Mp3DirectSink::new(file));

        let session_weak = Arc::downgrade(self);
        let on_mic_busy = Arc::new(move || {
            if let Some(session) = session_weak.upgrade() {
                session.set_state(RecordState::Error {
                    reason: "mic_busy".into(),
                });
                session.stop_and_shutdown(true);
            }
        });

        let mut pipeline = AudioPipeline::new(
            audio_source,
            encoder,
            None, // Record-only mode: transport is None
            Some(sink),
            *self.mic_gain.lock(),
            on_mic_busy,
            None,
        );

        pipeline.start()?;
        *self.pipeline.lock() = Some(pipeline);
        *self.recording_path.lock() = Some(display_path.clone());

        self.set_state(RecordState::Recording {
            path: display_path.clone(),
        });

        Ok(display_path)
    }

    fn set_state(&self, new_state: RecordState) {
        *self.current_state.lock() = new_state.clone();
        (self.state_callback)(new_state);
    }

    pub fn stop_and_shutdown(&self, preserve_terminal: bool) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }

        if let Some(mut p) = self.pipeline.lock().take() {
            p.stop();
        }

        let terminal = resolve_record_teardown_state(&self.current_state.lock(), preserve_terminal);
        self.set_state(terminal);
    }
}

fn chrono_or_system_time() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now();
    let duration = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    // Standard timestamp representation (seconds and millis)
    let s = secs % 86400;
    let hours = s / 3600;
    let minutes = (s % 3600) / 60;
    let seconds = s % 60;
    format!("{:02}{:02}{:02}-{:03}", hours, minutes, seconds, millis)
}

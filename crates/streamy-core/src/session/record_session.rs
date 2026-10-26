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

    /// Generate standard filename: `streamy-<yyyyMMdd-HHmmss-sss>.mp3`.
    pub fn generate_filename() -> String {
        format!("streamy-{}.mp3", timestamp_part())
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

/// UTC timestamp `yyyyMMdd-HHmmss-sss` from the system clock. Stdlib only, no chrono.
fn timestamp_part() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (y, m, dd) = civil_from_days((d.as_secs() / 86_400) as i64);
    let s = d.as_secs() % 86_400;
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{:03}",
        y, m, dd,
        s / 3600,
        (s % 3600) / 60,
        s % 60,
        d.subsec_millis()
    )
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(364), (1970, 12, 31));
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        // Leap-year boundaries (day numbers verified against Python date).
        assert_eq!(civil_from_days(10_956), (1999, 12, 31));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1)); // 2000 IS a leap year
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn generated_filename_has_date_and_correct_shape() {
        let a = RecordSession::generate_filename();
        // streamy-yyyyMMdd-HHmmss-sss.mp3 = 8 + 19 + 4
        assert_eq!(a.len(), 31);
        assert!(a.starts_with("streamy-20"));
        // Uniqueness across calls is only guaranteed to the millisecond; same-ms
        // calls are legal and the date prefix still prevents day-scale collisions.
    }
}

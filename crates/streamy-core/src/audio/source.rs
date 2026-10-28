//! AudioSource trait: abstracts PCM capture so tests can feed sine/WAV data with no sound card.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Receives captured PCM; returning false stops capture.
pub type FrameCallback = Box<dyn FnMut(&[i16]) -> bool + Send>;

pub trait AudioSource: Send + Sync + 'static {
    /// Start capturing audio. Calls `on_frame` repeatedly with captured 16-bit PCM samples.
    /// If `on_frame` returns false, capture stops.
    fn start(&mut self, on_frame: FrameCallback) -> Result<(), String>;

    /// Stop capture and release hardware resources. Idempotent.
    fn stop(&mut self);
}

/// A synthetic sine-wave audio source for tests and headless CI environments.
pub struct SineAudioSource {
    sample_rate_hz: u32,
    channels: u8,
    frame_samples: usize,
    running: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl SineAudioSource {
    pub fn new(sample_rate_hz: u32, mono: bool, frame_samples: usize) -> Self {
        Self {
            sample_rate_hz,
            channels: if mono { 1 } else { 2 },
            frame_samples,
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }
}

impl AudioSource for SineAudioSource {
    fn start(&mut self, mut on_frame: Box<dyn FnMut(&[i16]) -> bool + Send>) -> Result<(), String> {
        self.stop();
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let rate = self.sample_rate_hz;
        let channels = self.channels as usize;
        let frame_samples = self.frame_samples;

        let handle = thread::spawn(move || {
            let mut sample_idx = 0usize;
            let interval = Duration::from_micros(
                ((frame_samples as u64 * 1_000_000) / (rate as u64 * channels as u64)).max(1000),
            );

            while running.load(Ordering::SeqCst) {
                let mut buf = vec![0i16; frame_samples];
                for i in 0..frame_samples / channels {
                    let t = (sample_idx + i) as f32 / rate as f32;
                    let val = (t * 440.0 * 2.0 * std::f32::consts::PI).sin();
                    let s = (val * 16384.0) as i16;
                    for c in 0..channels {
                        buf[i * channels + c] = s;
                    }
                }
                sample_idx += frame_samples / channels;

                if !on_frame(&buf) {
                    break;
                }
                thread::sleep(interval);
            }
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.thread_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for SineAudioSource {
    fn drop(&mut self) {
        self.stop();
    }
}

//! AudioPipeline: glues AudioSource -> software gain -> LevelTap -> Mp3Encoder -> StreamTransport / StreamSink.
//! Operates on the audio thread with lock-free/mutex-guarded frame boundaries.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;

use crate::audio::sink::StreamSink;
use crate::audio::source::AudioSource;
use crate::core::level::{LevelSnapshot, LevelTap};
use crate::net::transport::StreamTransport;
use streamy_lame::Mp3Encoder;

const MIN_USEFUL_SAMPLES: usize = 160;
const SHORT_READ_LIMIT: usize = 50; // ~1s of persistent short reads
/// No audio callback for this long means the device vanished (unplugged,
/// disabled, or grabbed exclusively by another app).
const CAPTURE_STALL_MS: u64 = 4_000;
const WATCHDOG_TICK_MS: u64 = 250;

pub type LevelFeedFn = Arc<dyn Fn(LevelSnapshot) + Send + Sync>;
/// Called (off the audio thread) with a reason code when capture can't continue:
/// `mic_busy` (persistent short reads) or `mic_lost` (callbacks stopped).
pub type MicBusyFn = Arc<dyn Fn(String) + Send + Sync>;

pub struct AudioPipeline {
    capture: Box<dyn AudioSource>,
    encoder: Arc<Mutex<Mp3Encoder>>,
    transport: Option<Arc<StreamTransport>>,
    active_sink: Arc<Mutex<Option<Box<dyn StreamSink>>>>,
    pending_sink: Arc<Mutex<Option<Box<dyn StreamSink>>>>,
    stop_sink_requested: Arc<AtomicBool>,
    mic_gain: Arc<Mutex<f32>>,
    on_mic_busy: MicBusyFn,
    level_feed: Option<LevelFeedFn>,
    running: Arc<AtomicBool>,
    capture_failed: Arc<AtomicBool>,
    watchdog: Option<thread::JoinHandle<()>>,
}

impl AudioPipeline {
    pub fn new(
        capture: Box<dyn AudioSource>,
        encoder: Mp3Encoder,
        transport: Option<Arc<StreamTransport>>,
        sink: Option<Box<dyn StreamSink>>,
        initial_gain: f32,
        on_mic_busy: MicBusyFn,
        level_feed: Option<LevelFeedFn>,
    ) -> Self {
        Self {
            capture,
            encoder: Arc::new(Mutex::new(encoder)),
            transport,
            active_sink: Arc::new(Mutex::new(sink)),
            pending_sink: Arc::new(Mutex::new(None)),
            stop_sink_requested: Arc::new(AtomicBool::new(false)),
            mic_gain: Arc::new(Mutex::new(initial_gain)),
            on_mic_busy,
            level_feed,
            running: Arc::new(AtomicBool::new(false)),
            capture_failed: Arc::new(AtomicBool::new(false)),
            watchdog: None,
        }
    }

    pub fn set_gain(&self, gain: f32) {
        *self.mic_gain.lock() = gain;
    }

    pub fn arm_record(&self, new_sink: Box<dyn StreamSink>) {
        let mut pending = self.pending_sink.lock();
        if let Some(mut old) = pending.take() {
            old.flush();
            old.close();
        }
        *pending = Some(new_sink);
        self.stop_sink_requested.store(false, Ordering::SeqCst);
    }

    pub fn disarm_record(&self) {
        // Drop a sink that was armed but never swapped in, too.
        if let Some(mut s) = self.pending_sink.lock().take() {
            s.flush();
            s.close();
        }
        self.stop_sink_requested.store(true, Ordering::SeqCst);
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        self.capture_failed.store(false, Ordering::SeqCst);
        let encoder = self.encoder.clone();
        let transport = self.transport.clone();
        let active_sink = self.active_sink.clone();
        let pending_sink = self.pending_sink.clone();
        let stop_sink_requested = self.stop_sink_requested.clone();
        let mic_gain = self.mic_gain.clone();
        let on_mic_busy = self.on_mic_busy.clone();
        let level_feed = self.level_feed.clone();
        let running = self.running.clone();
        let capture_failed = self.capture_failed.clone();

        let epoch = Instant::now();
        let last_frame_ms = Arc::new(AtomicU64::new(0));
        let last_frame_cb = last_frame_ms.clone();

        let mut consecutive_short_reads = 0;

        self.capture.start(Box::new(move |raw_pcm| {
            if !running.load(Ordering::SeqCst) {
                return false;
            }
            last_frame_cb.store(epoch.elapsed().as_millis() as u64, Ordering::Relaxed);

            // 1) Swap-in or disarm record sink at frame boundary
            {
                let mut pending = pending_sink.lock();
                if let Some(new_s) = pending.take() {
                    let mut active = active_sink.lock();
                    if let Some(mut old) = active.take() {
                        old.flush();
                        old.close();
                    }
                    *active = Some(new_s);
                }
            }

            if stop_sink_requested.swap(false, Ordering::SeqCst) {
                let mut active = active_sink.lock();
                if let Some(mut s) = active.take() {
                    s.flush();
                    s.close();
                }
            }

            // 2) Mic contention / short reads check
            if raw_pcm.len() < MIN_USEFUL_SAMPLES {
                consecutive_short_reads += 1;
                if consecutive_short_reads > SHORT_READ_LIMIT {
                    // Never tear down from the audio thread: stop() pauses the
                    // CPAL stream, which joins this very callback. Deadlock.
                    if !capture_failed.swap(true, Ordering::SeqCst) {
                        let cb = on_mic_busy.clone();
                        thread::spawn(move || cb("mic_busy".into()));
                    }
                    return false;
                }
            } else {
                consecutive_short_reads = 0;
            }

            // 3) Apply software mic gain in-place
            let mut pcm = raw_pcm.to_vec();
            let gain = *mic_gain.lock();
            if gain != 1.0 {
                for sample in &mut pcm {
                    let amplified = (*sample as f32 * gain) as i32;
                    *sample = amplified.clamp(-32768, 32767) as i16;
                }
            }

            // 4) Pre-encoder LevelTap
            if let Some(ref feed) = level_feed {
                let snapshot = LevelSnapshot {
                    samples: Vec::new(),
                    rms_db: LevelTap::rms_db(&pcm),
                    peak_db: LevelTap::peak_db(&pcm),
                };
                feed(snapshot);
            }

            // 5) Encode frame with LAME
            let frames = {
                let mut enc = encoder.lock();
                enc.encode(&pcm).unwrap_or_default()
            };

            // 6) Send to transport if streaming
            if let Some(ref t) = transport {
                for frame in &frames {
                    t.enqueue(frame.clone());
                }
            }

            // 7) Pass to active sink if recording
            {
                let mut sink_guard = active_sink.lock();
                if let Some(ref mut sink) = *sink_guard {
                    sink.on_pcm(&pcm);
                    sink.on_encoded(&frames);
                }
            }

            true
        }))?;

        // Watchdog: a capture device that stops delivering callbacks (USB mic
        // unplugged, device disabled) produces no error on most backends.
        let running = self.running.clone();
        let capture_failed = self.capture_failed.clone();
        let on_mic_busy = self.on_mic_busy.clone();
        self.watchdog = Some(
            thread::Builder::new()
                .name("streamy-capture-watchdog".into())
                .spawn(move || {
                    while running.load(Ordering::SeqCst) {
                        thread::sleep(Duration::from_millis(WATCHDOG_TICK_MS));
                        let now = epoch.elapsed().as_millis() as u64;
                        let last = last_frame_ms.load(Ordering::Relaxed);
                        if running.load(Ordering::SeqCst) && now.saturating_sub(last) > CAPTURE_STALL_MS {
                            if !capture_failed.swap(true, Ordering::SeqCst) {
                                let cb = on_mic_busy.clone();
                                thread::spawn(move || cb("mic_lost".into()));
                            }
                            return;
                        }
                    }
                })
                .map_err(|e| format!("Failed to start capture watchdog: {e}"))?,
        );
        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(w) = self.watchdog.take() {
            let _ = w.join();
        }
        self.capture.stop();

        if self.encoder.lock().is_closed() {
            return; // already stopped (Drop after an explicit stop)
        }

        // Flush trailing frames from encoder
        let trailing = {
            let mut enc = self.encoder.lock();
            enc.flush().unwrap_or_default()
        };

        if let Some(ref t) = self.transport {
            for f in &trailing {
                t.enqueue(f.clone());
            }
        }
        // Trailing output also completes the local recording, if any.
        {
            let mut sink_guard = self.active_sink.lock();
            if let Some(ref mut s) = *sink_guard {
                s.on_encoded(&trailing);
            }
        }

        // Close both active and pending sinks
        {
            let mut active = self.active_sink.lock();
            if let Some(mut s) = active.take() {
                s.flush();
                s.close();
            }
        }
        {
            let mut pending = self.pending_sink.lock();
            if let Some(mut s) = pending.take() {
                s.flush();
                s.close();
            }
        }

        // Close encoder
        self.encoder.lock().close();
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        self.stop();
    }
}

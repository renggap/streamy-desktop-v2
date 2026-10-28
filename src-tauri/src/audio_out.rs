//! Local monitoring of the playlist through the default output device.
//!
//! The player's clock pushes blocks into a short ring buffer; the device
//! callback drains it. The buffer is capped so monitoring can lag the stream by
//! at most a quarter second, and overflow drops the oldest audio.

use std::collections::VecDeque;
use std::sync::Arc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use parking_lot::Mutex;

use streamy_core::audio::decoder::{PLAYER_CHANNELS, PLAYER_RATE};
use streamy_core::audio::player::PlaylistPlayer;
use streamy_core::audio::resample::CaptureConverter;

const MAX_BUFFER_MS: usize = 250;
/// Wait for this much audio before (re)starting output, to ride out jitter.
const PRIME_MS: usize = 60;

struct Ring {
    buf: VecDeque<f32>,
    cap: usize,
    prime: usize,
    primed: bool,
}

pub struct MonitorOutput {
    player: Arc<PlaylistPlayer>,
    tap: Option<u64>,
    stream: Option<cpal::Stream>,
}

// cpal::Stream is !Send on some platforms; AppState only touches it under its mutex.
unsafe impl Send for MonitorOutput {}
unsafe impl Sync for MonitorOutput {}

fn build<T>(device: &cpal::Device, config: &StreamConfig, ring: Arc<Mutex<Ring>>) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| {
                let mut r = ring.lock();
                if !r.primed && r.buf.len() >= r.prime {
                    r.primed = true;
                }
                for s in out.iter_mut() {
                    let v = if r.primed { r.buf.pop_front() } else { None };
                    if v.is_none() {
                        r.primed = false;
                    }
                    *s = T::from_sample(v.unwrap_or(0.0));
                }
            },
            |err| tracing::warn!("monitor output error: {err}"),
            None,
        )
        .map_err(|e| format!("Cannot open speakers: {e}"))
}

impl MonitorOutput {
    pub fn start(player: Arc<PlaylistPlayer>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No speakers or headphones found".to_string())?;
        let default_cfg = device
            .default_output_config()
            .map_err(|e| format!("Cannot read speaker format: {e}"))?;
        let format = default_cfg.sample_format();
        let config: StreamConfig = default_cfg.into();
        let rate = config.sample_rate.0;
        let channels = config.channels as usize;

        let per_ms = rate as usize * channels / 1000;
        let ring = Arc::new(Mutex::new(Ring {
            buf: VecDeque::with_capacity(per_ms * MAX_BUFFER_MS),
            cap: per_ms * MAX_BUFFER_MS,
            prime: per_ms * PRIME_MS,
            primed: false,
        }));

        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, &config, ring.clone()),
            SampleFormat::I16 => build::<i16>(&device, &config, ring.clone()),
            SampleFormat::U16 => build::<u16>(&device, &config, ring.clone()),
            SampleFormat::I32 => build::<i32>(&device, &config, ring.clone()),
            other => Err(format!("Unsupported speaker sample format: {other}")),
        }?;
        stream.play().map_err(|e| format!("Cannot start speakers: {e}"))?;

        let mut converter = CaptureConverter::new(PLAYER_RATE, PLAYER_CHANNELS, rate, channels)?;
        let tap = player.add_tap(Box::new(move |block| {
            let samples = converter.process_f32(block);
            let mut r = ring.lock();
            r.buf.extend(samples);
            let excess = r.buf.len().saturating_sub(r.cap);
            r.buf.drain(..excess);
        }));

        Ok(Self {
            player,
            tap: Some(tap),
            stream: Some(stream),
        })
    }

    pub fn stop(&mut self) {
        if let Some(id) = self.tap.take() {
            self.player.remove_tap(id);
        }
        if let Some(s) = self.stream.take() {
            let _ = s.pause();
        }
    }
}

impl Drop for MonitorOutput {
    fn drop(&mut self) {
        self.stop();
    }
}

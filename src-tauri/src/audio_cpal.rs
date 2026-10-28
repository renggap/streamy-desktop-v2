//! cpal WASAPI implementation of AudioSource for Windows.
//! Accepts any common device sample format, then noise suppression and
//! downmix/upmix + rubato resampling to the encoder's format via `MicProcessor`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use serde::{Deserialize, Serialize};

use streamy_core::audio::denoise::MicProcessor;
use streamy_core::audio::source::{AudioSource, FrameCallback};
use streamy_core::audio::switch::InputControl;

/// Frames handed to the pipeline per call (~23 ms at 44.1 kHz).
const EMIT_FRAMES: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceDesc {
    pub name: String,
    pub is_default: bool,
}

pub struct CpalAudioSource {
    device_name: Option<String>,
    target_sample_rate: u32,
    target_mono: bool,
    control: Arc<InputControl>,
    stream: Option<cpal::Stream>,
    running: Arc<AtomicBool>,
}

impl CpalAudioSource {
    /// `control` supplies the live noise-suppression level.
    pub fn new(device_name: Option<String>, target_sample_rate: u32, target_mono: bool, control: Arc<InputControl>) -> Self {
        Self {
            device_name,
            target_sample_rate,
            target_mono,
            control,
            stream: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Enumerate all available audio input devices.
    pub fn list_devices() -> Vec<AudioDeviceDesc> {
        let host = cpal::default_host();
        let default_name = host.default_input_device().and_then(|d| d.name().ok());

        let mut out = Vec::new();
        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    let is_default = default_name.as_deref() == Some(&name);
                    out.push(AudioDeviceDesc { name, is_default });
                }
            }
        }
        out
    }

    fn find_device(&self, host: &cpal::Host) -> Result<cpal::Device, String> {
        if let Some(name) = &self.device_name {
            if let Ok(mut devices) = host.input_devices() {
                if let Some(d) = devices.find(|d| d.name().map(|n| n == *name).unwrap_or(false)) {
                    return Ok(d);
                }
            }
            // Device names change when a USB mic moves ports; the default
            // device beats refusing to go on air.
            tracing::warn!("input device {name:?} not found, using the default device");
        }
        host.default_input_device().ok_or_else(|| "mic_not_found".to_string())
    }
}

// cpal::Stream contains platform-specific pointers that are marked !Send / !Sync on some platforms (e.g. ALSA).
// In Streamy, the CpalAudioSource manages the stream lifecycle exclusively within a single mutex / session owner.
unsafe impl Send for CpalAudioSource {}
unsafe impl Sync for CpalAudioSource {}

fn map_device_error(context: &str, e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("access") || lower.contains("denied") || lower.contains("permission") {
        "mic_denied".to_string()
    } else if lower.contains("not available") || lower.contains("no longer") {
        "mic_not_found".to_string()
    } else {
        format!("{context}: {msg}")
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut converter: MicProcessor,
    out_channels: usize,
    running: Arc<AtomicBool>,
    mut on_frame: FrameCallback,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let mut scratch: Vec<f32> = Vec::new();
    let mut buffer: Vec<i16> = Vec::new();
    let slice = EMIT_FRAMES * out_channels;

    device.build_input_stream(
        config,
        move |data: &[T], _| {
            if !running.load(Ordering::SeqCst) {
                return;
            }
            scratch.clear();
            scratch.extend(data.iter().map(|s| s.to_sample::<f32>()));
            buffer.extend(converter.process(&scratch));
            while buffer.len() >= slice {
                let keep_going = on_frame(&buffer[..slice]);
                buffer.drain(..slice);
                if !keep_going {
                    running.store(false, Ordering::SeqCst);
                    return;
                }
            }
        },
        // Device loss mostly shows up as silence (no callbacks), which the
        // pipeline watchdog turns into `mic_lost`; log the cause here.
        |err: cpal::StreamError| tracing::error!("audio capture error: {err}"),
        None,
    )
}

impl AudioSource for CpalAudioSource {
    fn start(&mut self, on_frame: FrameCallback) -> Result<(), String> {
        self.stop();

        let host = cpal::default_host();
        let device = self.find_device(&host)?;
        if let Ok(name) = device.name() {
            tracing::info!("capturing from {name:?}");
        }

        let default_cfg = device
            .default_input_config()
            .map_err(|e| map_device_error("Cannot read microphone format", e))?;

        let sample_format = default_cfg.sample_format();
        let config: StreamConfig = default_cfg.into();

        let in_rate = config.sample_rate.0;
        let in_channels = config.channels as usize;
        let out_channels = if self.target_mono { 1 } else { 2 };
        let converter = MicProcessor::new(in_rate, in_channels, self.target_sample_rate, out_channels, self.control.clone())?;

        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();

        macro_rules! build {
            ($t:ty) => {
                build_stream::<$t>(&device, &config, converter, out_channels, running, on_frame)
            };
        }
        let stream = match sample_format {
            SampleFormat::F32 => build!(f32),
            SampleFormat::F64 => build!(f64),
            SampleFormat::I16 => build!(i16),
            SampleFormat::I32 => build!(i32),
            SampleFormat::I8 => build!(i8),
            SampleFormat::U16 => build!(u16),
            SampleFormat::U8 => build!(u8),
            SampleFormat::U32 => build!(u32),
            other => {
                self.running.store(false, Ordering::SeqCst);
                return Err(format!("Unsupported microphone sample format: {other}"));
            }
        }
        .map_err(|e| {
            self.running.store(false, Ordering::SeqCst);
            map_device_error("Cannot open microphone", e)
        })?;

        stream.play().map_err(|e| {
            self.running.store(false, Ordering::SeqCst);
            map_device_error("Cannot start microphone", e)
        })?;

        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(stream) = self.stream.take() {
            let _ = stream.pause();
            // Dropping the stream releases the device.
        }
    }
}

impl Drop for CpalAudioSource {
    fn drop(&mut self) {
        self.stop();
    }
}

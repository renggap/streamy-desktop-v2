//! cpal WASAPI implementation of AudioSource for Windows.
//! Handles mix formats (f32/i16), channel downmix/upmix, rubato resampling, and mic_denied privacy handling.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use serde::{Deserialize, Serialize};

use streamy_core::audio::resample::{f32_to_i16, mono_to_stereo_i16, stereo_to_mono_i16, AudioResampler};
use streamy_core::audio::source::AudioSource;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceDesc {
    pub name: String,
    pub is_default: bool,
}

pub struct CpalAudioSource {
    device_name: Option<String>,
    target_sample_rate: u32,
    target_mono: bool,
    stream: Option<cpal::Stream>,
    running: Arc<AtomicBool>,
}

impl CpalAudioSource {
    pub fn new(device_name: Option<String>, target_sample_rate: u32, target_mono: bool) -> Self {
        Self {
            device_name,
            target_sample_rate,
            target_mono,
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
}

// cpal::Stream contains platform-specific pointers that are marked !Send / !Sync on some platforms (e.g. ALSA).
// In Streamy, the CpalAudioSource manages the stream lifecycle exclusively within a single mutex / session owner.
unsafe impl Send for CpalAudioSource {}
unsafe impl Sync for CpalAudioSource {}

impl AudioSource for CpalAudioSource {
    fn start(&mut self, mut on_frame: Box<dyn FnMut(&[i16]) -> bool + Send>) -> Result<(), String> {
        self.stop();

        let host = cpal::default_host();
        let device = match &self.device_name {
            Some(name) => {
                let mut found = None;
                if let Ok(devices) = host.input_devices() {
                    for d in devices {
                        if d.name().map(|n| n == *name).unwrap_or(false) {
                            found = Some(d);
                            break;
                        }
                    }
                }
                found.or_else(|| host.default_input_device())
            }
            None => host.default_input_device(),
        };

        let device = device.ok_or_else(|| "mic_denied".to_string())?;

        let default_cfg = device
            .default_input_config()
            .map_err(|e| {
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("access") || err_str.contains("denied") || err_str.contains("permission") {
                    "mic_denied".to_string()
                } else {
                    format!("Failed to get default input config: {:?}", e)
                }
            })?;

        let sample_format = default_cfg.sample_format();
        let config: StreamConfig = default_cfg.into();

        let in_rate = config.sample_rate.0;
        let in_channels = config.channels as usize;
        let target_rate = self.target_sample_rate;
        let target_mono = self.target_mono;
        let target_channels = if target_mono { 1 } else { 2 };

        let mut resampler = AudioResampler::new(in_rate, target_rate, in_channels)
            .map_err(|e| format!("Resampler error: {:?}", e))?;

        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();

        let err_fn = move |err: cpal::StreamError| {
            tracing::error!("CPAL audio capture error: {:?}", err);
        };

        let stream = match sample_format {
            SampleFormat::F32 => {
                let mut buffer_pcm: Vec<i16> = Vec::new();

                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        if !running.load(Ordering::SeqCst) {
                            return;
                        }

                        // 1) Resample if needed
                        let resampled_f32 = match resampler.process_interleaved_f32(data) {
                            Ok(r) => r,
                            Err(_) => data.to_vec(),
                        };

                        // 2) Convert f32 to i16
                        let mut pcm16 = f32_to_i16(&resampled_f32);

                        // 3) Channel conversion (match target_channels)
                        if in_channels == 1 && target_channels == 2 {
                            pcm16 = mono_to_stereo_i16(&pcm16);
                        } else if in_channels >= 2 && target_channels == 1 {
                            pcm16 = stereo_to_mono_i16(&pcm16);
                        }

                        // 4) Emit in ~1024 frame slices
                        buffer_pcm.extend(pcm16);
                        let slice_size = 1024 * target_channels;
                        while buffer_pcm.len() >= slice_size {
                            let chunk: Vec<i16> = buffer_pcm.drain(..slice_size).collect();
                            if !on_frame(&chunk) {
                                running.store(false, Ordering::SeqCst);
                                break;
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::I16 => {
                let mut buffer_pcm: Vec<i16> = Vec::new();

                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        if !running.load(Ordering::SeqCst) {
                            return;
                        }

                        let mut pcm16 = data.to_vec();

                        // Channel conversion
                        if in_channels == 1 && target_channels == 2 {
                            pcm16 = mono_to_stereo_i16(&pcm16);
                        } else if in_channels >= 2 && target_channels == 1 {
                            pcm16 = stereo_to_mono_i16(&pcm16);
                        }

                        buffer_pcm.extend(pcm16);
                        let slice_size = 1024 * target_channels;
                        while buffer_pcm.len() >= slice_size {
                            let chunk: Vec<i16> = buffer_pcm.drain(..slice_size).collect();
                            if !on_frame(&chunk) {
                                running.store(false, Ordering::SeqCst);
                                break;
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            _ => return Err("Unsupported CPAL sample format".to_string()),
        }
        .map_err(|e| {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("access") || err_str.contains("denied") || err_str.contains("permission") {
                "mic_denied".to_string()
            } else {
                format!("Failed to build CPAL input stream: {:?}", e)
            }
        })?;

        stream
            .play()
            .map_err(|e| format!("Failed to play CPAL stream: {:?}", e))?;

        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(stream) = self.stream.take() {
            let _ = stream.pause();
        }
    }
}

impl Drop for CpalAudioSource {
    fn drop(&mut self) {
        self.stop();
    }
}

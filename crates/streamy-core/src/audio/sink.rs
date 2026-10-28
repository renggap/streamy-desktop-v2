//! Stream sinks for local MP3 recording.

use std::fs::File;
use std::io::Write;
use parking_lot::Mutex;
use streamy_lame::Mp3Encoder;

use crate::audio::resample::CaptureConverter;

pub trait StreamSink: Send + Sync {
    fn on_pcm(&mut self, pcm: &[i16]);
    fn on_encoded(&mut self, frames: &[Vec<u8>]);
    fn flush(&mut self);
    fn close(&mut self);
}

/// Direct MP3 pass-through sink: writes broadcast MP3 frames as-is with zero re-encoding.
pub struct Mp3DirectSink {
    file: Option<File>,
}

impl Mp3DirectSink {
    pub fn new(file: File) -> Self {
        Self { file: Some(file) }
    }
}

impl StreamSink for Mp3DirectSink {
    fn on_pcm(&mut self, _pcm: &[i16]) {
        // No-op for direct pass-through
    }

    fn on_encoded(&mut self, frames: &[Vec<u8>]) {
        if let Some(ref mut f) = self.file {
            for frame in frames {
                let _ = f.write_all(frame);
            }
        }
    }

    fn flush(&mut self) {
        if let Some(ref mut f) = self.file {
            let _ = f.flush();
        }
    }

    fn close(&mut self) {
        self.flush();
        self.file = None;
    }
}

/// Records at its own quality: re-encodes the pipeline's PCM with a separate
/// encoder (resampling/remixing first when the formats differ), so a low-bitrate
/// broadcast can still leave a high-bitrate archive.
pub struct Mp3EncodeSink {
    file: Option<File>,
    encoder: Mp3Encoder,
    /// Mutex only for `Sync` (the resampler isn't); the audio thread is the sole user.
    converter: Option<Mutex<CaptureConverter>>,
    scratch: Vec<f32>,
}

impl Mp3EncodeSink {
    /// `in_rate`/`in_channels` describe the PCM the pipeline delivers.
    pub fn new(
        file: File,
        in_rate: u32,
        in_channels: usize,
        out_rate: u32,
        bitrate_kbps: u32,
        mono: bool,
    ) -> Result<Self, String> {
        let encoder = Mp3Encoder::new(out_rate, bitrate_kbps, mono)
            .map_err(|e| format!("Failed to create recording encoder: {e:?}"))?;
        let out_channels = if mono { 1 } else { 2 };
        let converter = if in_rate == out_rate && in_channels == out_channels {
            None
        } else {
            Some(Mutex::new(CaptureConverter::new(in_rate, in_channels, out_rate, out_channels)?))
        };
        Ok(Self {
            file: Some(file),
            encoder,
            converter,
            scratch: Vec::new(),
        })
    }

    fn write(&mut self, frames: &[Vec<u8>]) {
        if let Some(ref mut f) = self.file {
            for frame in frames {
                let _ = f.write_all(frame);
            }
        }
    }
}

impl StreamSink for Mp3EncodeSink {
    fn on_pcm(&mut self, pcm: &[i16]) {
        if self.file.is_none() {
            return;
        }
        let frames = match self.converter.as_mut() {
            None => self.encoder.encode(pcm),
            Some(conv) => {
                self.scratch.clear();
                self.scratch.extend(pcm.iter().map(|&s| s as f32 / 32_768.0));
                let converted = conv.get_mut().process(&self.scratch);
                self.encoder.encode(&converted)
            }
        };
        let frames = frames.unwrap_or_default();
        self.write(&frames);
    }

    fn on_encoded(&mut self, _frames: &[Vec<u8>]) {
        // Ignores the broadcast's frames; this sink encodes its own.
    }

    fn flush(&mut self) {
        if let Some(ref mut f) = self.file {
            let _ = f.flush();
        }
    }

    fn close(&mut self) {
        if self.file.is_some() {
            let tail = self.encoder.flush().unwrap_or_default();
            self.write(&tail);
            self.encoder.close();
        }
        self.flush();
        self.file = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MPEG audio frame header at the start of `data`: (bitrate kbps, sample rate, mono).
    fn first_header(data: &[u8]) -> (u32, u32, bool) {
        let i = data.windows(2).position(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0).expect("no frame sync");
        let h = &data[i..i + 4];
        assert_eq!(h[1] & 0x18, 0x18, "expected MPEG-1");
        const KBPS: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
        const RATES: [u32; 3] = [44_100, 48_000, 32_000];
        (KBPS[(h[2] >> 4) as usize], RATES[((h[2] >> 2) & 3) as usize], h[3] >> 6 == 3)
    }

    #[test]
    fn re_encodes_at_its_own_bitrate_rate_and_channels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.mp3");
        // Broadcast PCM: 22.05 kHz mono. Recording: 44.1 kHz stereo at 320 kbps.
        let mut sink = Mp3EncodeSink::new(File::create(&path).unwrap(), 22_050, 1, 44_100, 320, false).unwrap();
        let tone: Vec<i16> = (0..1024).map(|i| ((i as f32 * 0.1).sin() * 8000.0) as i16).collect();
        for _ in 0..40 {
            sink.on_pcm(&tone);
            sink.on_encoded(&[b"BROADCAST-FRAME".to_vec()]); // broadcast frames must not leak in
        }
        sink.flush();
        sink.close();
        let data = std::fs::read(&path).unwrap();
        assert!(!data.windows(15).any(|w| w == b"BROADCAST-FRAME"));
        assert_eq!(first_header(&data), (320, 44_100, false));
        // ~1.86 s at 320 kbps is ~74 KB.
        assert!((50_000..100_000).contains(&data.len()), "{}", data.len());
    }
}

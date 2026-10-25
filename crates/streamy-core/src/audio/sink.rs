//! Stream sinks for local MP3 recording: direct pass-through and transcode sinks.

use std::fs::File;
use std::io::Write;
use streamy_lame::Mp3Encoder;

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

/// Transcode sink: encodes raw PCM to MP3 using a dedicated LAME encoder instance.
pub struct Mp3TranscodeSink {
    file: Option<File>,
    encoder: Option<Mp3Encoder>,
}

impl Mp3TranscodeSink {
    pub fn new(file: File, sample_rate_hz: u32, mono: bool) -> Result<Self, String> {
        let encoder = Mp3Encoder::new(sample_rate_hz, 128, mono)
            .map_err(|e| format!("Failed to initialize LAME transcode encoder: {:?}", e))?;
        Ok(Self {
            file: Some(file),
            encoder: Some(encoder),
        })
    }
}

impl StreamSink for Mp3TranscodeSink {
    fn on_pcm(&mut self, pcm: &[i16]) {
        if let (Some(ref mut enc), Some(ref mut file)) = (&mut self.encoder, &mut self.file) {
            if let Ok(frames) = enc.encode(pcm) {
                for frame in frames {
                    let _ = file.write_all(&frame);
                }
            }
        }
    }

    fn on_encoded(&mut self, _frames: &[Vec<u8>]) {
        // No-op: consumes raw PCM
    }

    fn flush(&mut self) {
        if let (Some(ref mut enc), Some(ref mut file)) = (&mut self.encoder, &mut self.file) {
            if let Ok(frames) = enc.flush() {
                for frame in frames {
                    let _ = file.write_all(&frame);
                }
            }
            let _ = file.flush();
        }
    }

    fn close(&mut self) {
        self.flush();
        if let Some(mut enc) = self.encoder.take() {
            enc.close();
        }
        self.file = None;
    }
}

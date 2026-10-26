//! Stream sinks for local MP3 recording.

use std::fs::File;
use std::io::Write;

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

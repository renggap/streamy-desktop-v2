use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_uchar};
use thiserror::Error;

#[repr(C)]
pub struct LameGlobalFlags {
    _private: [u8; 0],
}

pub type LameHandle = *mut LameGlobalFlags;

extern "C" {
    pub fn lame_init() -> LameHandle;
    pub fn lame_set_in_samplerate(handle: LameHandle, rate: c_int) -> c_int;
    pub fn lame_set_num_channels(handle: LameHandle, channels: c_int) -> c_int;
    pub fn lame_set_out_samplerate(handle: LameHandle, rate: c_int) -> c_int;
    pub fn lame_set_brate(handle: LameHandle, bitrate_kbps: c_int) -> c_int;
    pub fn lame_set_quality(handle: LameHandle, quality: c_int) -> c_int;
    pub fn lame_init_params(handle: LameHandle) -> c_int;
    pub fn lame_get_out_samplerate(handle: LameHandle) -> c_int;

    pub fn lame_encode_buffer(
        handle: LameHandle,
        buffer_l: *const i16,
        buffer_r: *const i16,
        nsamples: c_int,
        mp3buf: *mut c_uchar,
        mp3buf_size: c_int,
    ) -> c_int;

    pub fn lame_encode_buffer_interleaved(
        handle: LameHandle,
        pcm: *const i16,
        num_samples: c_int,
        mp3buf: *mut c_uchar,
        mp3buf_size: c_int,
    ) -> c_int;

    pub fn lame_encode_flush(
        handle: LameHandle,
        mp3buf: *mut c_uchar,
        size: c_int,
    ) -> c_int;

    pub fn lame_close(handle: LameHandle) -> c_int;
    pub fn get_lame_version() -> *const c_char;
}

#[derive(Error, Debug)]
pub enum LameError {
    #[error("Failed to initialize LAME handle (lame_init returned NULL)")]
    InitFailed,
    #[error("lame_init_params failed with code {0}")]
    InitParamsFailed(i32),
    #[error("LAME output sample rate {actual} != configured {expected}")]
    SampleRateMismatch { actual: u32, expected: u32 },
    #[error("Encode attempted after encoder was closed")]
    AlreadyClosed,
    #[error("LAME encode buffer error with code {0}")]
    EncodeFailed(i32),
    #[error("LAME flush error with code {0}")]
    FlushFailed(i32),
}

/// Thread-confined, channel-aware MP3 encoder backed by vendored libmp3lame.
/// Mirrors `com.streamy.app.audio.Mp3Encoder` and `lame_jni.cpp`.
pub struct Mp3Encoder {
    handle: LameHandle,
    channels: u32,
    sample_rate_hz: u32,
    out_sample_rate_hz: u32,
    closed: bool,
}

// Safety: The handle methods require exclusive `&mut self` access.
unsafe impl Send for Mp3Encoder {}
unsafe impl Sync for Mp3Encoder {}

impl Mp3Encoder {
    pub const MIME: &'static str = "audio/mpeg";

    /// Initialize LAME with exact parameters matching Android's `lame_jni.cpp`:
    /// quality = 5, out_samplerate pinned to configured input rate.
    pub fn new(sample_rate_hz: u32, bitrate_kbps: u32, mono: bool) -> Result<Self, LameError> {
        let channels = if mono { 1 } else { 2 };
        unsafe {
            let handle = lame_init();
            if handle.is_null() {
                return Err(LameError::InitFailed);
            }

            lame_set_in_samplerate(handle, sample_rate_hz as c_int);
            lame_set_num_channels(handle, channels as c_int);
            lame_set_out_samplerate(handle, sample_rate_hz as c_int);
            lame_set_brate(handle, bitrate_kbps as c_int);
            lame_set_quality(handle, 5);

            let res = lame_init_params(handle);
            if res < 0 {
                lame_close(handle);
                return Err(LameError::InitParamsFailed(res));
            }

            let out_rate = lame_get_out_samplerate(handle) as u32;
            if out_rate != sample_rate_hz {
                lame_close(handle);
                return Err(LameError::SampleRateMismatch {
                    actual: out_rate,
                    expected: sample_rate_hz,
                });
            }

            Ok(Self {
                handle,
                channels,
                sample_rate_hz,
                out_sample_rate_hz: out_rate,
                closed: false,
            })
        }
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    pub fn out_sample_rate_hz(&self) -> u32 {
        self.out_sample_rate_hz
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Encode 16-bit PCM samples into MP3 frames.
    /// For mono, `pcm` is mono samples.
    /// For stereo, `pcm` is interleaved L,R,L,R,... samples.
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<Vec<u8>>, LameError> {
        if self.closed {
            return Err(LameError::AlreadyClosed);
        }
        if pcm.is_empty() {
            return Ok(Vec::new());
        }

        // Worst-case MP3 buffer size: 1.25x + 7200 bytes per frame (per LAME documentation)
        let max_out = (pcm.len() * 5) / 4 + 7200;
        let mut out_buf = vec![0u8; max_out];

        let written = unsafe {
            if self.channels == 1 {
                lame_encode_buffer(
                    self.handle,
                    pcm.as_ptr(),
                    pcm.as_ptr(),
                    pcm.len() as c_int,
                    out_buf.as_mut_ptr(),
                    max_out as c_int,
                )
            } else {
                let num_samples_per_chan = (pcm.len() / 2) as c_int;
                lame_encode_buffer_interleaved(
                    self.handle,
                    pcm.as_ptr(),
                    num_samples_per_chan,
                    out_buf.as_mut_ptr(),
                    max_out as c_int,
                )
            }
        };

        if written < 0 {
            return Err(LameError::EncodeFailed(written));
        }

        if written > 0 {
            out_buf.truncate(written as usize);
            Ok(vec![out_buf])
        } else {
            Ok(Vec::new())
        }
    }

    /// Drain any trailing buffered MP3 frames.
    pub fn flush(&mut self) -> Result<Vec<Vec<u8>>, LameError> {
        if self.closed {
            return Err(LameError::AlreadyClosed);
        }

        let mut out_buf = vec![0u8; 7200];
        let written = unsafe {
            lame_encode_flush(self.handle, out_buf.as_mut_ptr(), 7200)
        };

        if written < 0 {
            return Err(LameError::FlushFailed(written));
        }

        if written > 0 {
            out_buf.truncate(written as usize);
            Ok(vec![out_buf])
        } else {
            Ok(Vec::new())
        }
    }

    /// Close the encoder and free native LAME resources. Idempotent.
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            if !self.handle.is_null() {
                unsafe {
                    lame_close(self.handle);
                }
                self.handle = std::ptr::null_mut();
            }
        }
    }
}

impl Drop for Mp3Encoder {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn lame_version() -> &'static str {
    unsafe {
        let ptr = get_lame_version();
        if ptr.is_null() {
            "unknown"
        } else {
            CStr::from_ptr(ptr).to_str().unwrap_or("unknown")
        }
    }
}

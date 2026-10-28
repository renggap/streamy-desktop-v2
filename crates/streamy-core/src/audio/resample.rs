//! Audio sample format, channel layout, and sample-rate conversion (rubato).

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

/// Streaming capture converter: interleaved f32 at the device's rate/channel
/// layout -> interleaved i16 at the encoder's rate/channel layout.
///
/// Audio callbacks deliver arbitrary buffer sizes (WASAPI typically 10 ms), while
/// the sinc resampler consumes fixed-size chunks, so input is buffered per
/// channel and resampled whenever a full chunk is available.
pub struct CaptureConverter {
    in_channels: usize,
    out_channels: usize,
    resampler: Option<SincFixedIn<f32>>,
    pending: Vec<Vec<f32>>,
}

impl CaptureConverter {
    pub const CHUNK_FRAMES: usize = 480;

    pub fn new(in_rate: u32, in_channels: usize, out_rate: u32, out_channels: usize) -> Result<Self, String> {
        if in_channels == 0 || out_channels == 0 || in_rate == 0 || out_rate == 0 {
            return Err(format!(
                "invalid audio format: {in_rate} Hz / {in_channels} ch -> {out_rate} Hz / {out_channels} ch"
            ));
        }
        let resampler = if in_rate == out_rate {
            None
        } else {
            let params = SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            };
            Some(
                SincFixedIn::<f32>::new(
                    out_rate as f64 / in_rate as f64,
                    1.0,
                    params,
                    Self::CHUNK_FRAMES,
                    out_channels,
                )
                .map_err(|e| format!("Failed to create resampler: {e}"))?,
            )
        };
        Ok(Self {
            in_channels,
            out_channels,
            resampler,
            pending: vec![Vec::with_capacity(Self::CHUNK_FRAMES * 2); out_channels],
        })
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<i16> {
        // 1) Channel map into planar buffers (downmix = average, upmix = duplicate).
        let in_ch = self.in_channels;
        for frame in input.chunks_exact(in_ch) {
            if self.out_channels == 1 {
                let sum: f32 = frame.iter().sum();
                self.pending[0].push(sum / in_ch as f32);
            } else {
                let l = frame[0];
                let r = if in_ch > 1 { frame[1] } else { frame[0] };
                self.pending[0].push(l);
                self.pending[1].push(r);
            }
        }

        // 2) Resample whole chunks; leftovers wait for the next callback.
        let planar: Vec<Vec<f32>> = match &mut self.resampler {
            None => self.pending.iter_mut().map(std::mem::take).collect(),
            Some(r) => {
                let mut out = vec![Vec::new(); self.out_channels];
                loop {
                    let need = r.input_frames_next();
                    if self.pending[0].len() < need {
                        break;
                    }
                    let chunk: Vec<&[f32]> = self.pending.iter().map(|c| &c[..need]).collect();
                    if let Ok(res) = r.process(&chunk, None) {
                        for (o, c) in out.iter_mut().zip(res) {
                            o.extend_from_slice(&c);
                        }
                    }
                    for c in self.pending.iter_mut() {
                        c.drain(..need);
                    }
                }
                out
            }
        };

        // 3) Interleave + quantize.
        let frames = planar.first().map_or(0, |c| c.len());
        let mut out = Vec::with_capacity(frames * self.out_channels);
        for f in 0..frames {
            for c in &planar {
                out.push(f32_sample_to_i16(c[f]));
            }
        }
        out
    }
}

#[inline]
fn f32_sample_to_i16(s: f32) -> i16 {
    let mult = if s < 0.0 { 32768.0 } else { 32767.0 };
    (s * mult).clamp(-32768.0, 32767.0) as i16
}

/// Convert f32 PCM to i16 PCM in-place or returning a new buffer.
pub fn f32_to_i16(input: &[f32]) -> Vec<i16> {
    input
        .iter()
        .map(|&s| f32_sample_to_i16(s))
        .collect()
}

/// Convert mono i16 PCM to stereo i16 PCM by duplicating channels.
pub fn mono_to_stereo_i16(mono: &[i16]) -> Vec<i16> {
    let mut out = Vec::with_capacity(mono.len() * 2);
    for &s in mono {
        out.push(s);
        out.push(s);
    }
    out
}

/// Convert stereo i16 PCM to mono i16 PCM by averaging L + R.
pub fn stereo_to_mono_i16(stereo: &[i16]) -> Vec<i16> {
    let mut out = Vec::with_capacity(stereo.len() / 2);
    for chunk in stereo.chunks_exact(2) {
        let avg = ((chunk[0] as i32 + chunk[1] as i32) / 2) as i16;
        out.push(avg);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_conversions() {
        let f = vec![-1.0, 0.0, 0.5, 1.0];
        let i = f32_to_i16(&f);
        assert_eq!(i[0], -32768);
        assert_eq!(i[1], 0);
        assert_eq!(i[2], 16383);
        assert_eq!(i[3], 32767);
    }

    #[test]
    fn converter_resamples_odd_sized_callbacks_at_the_right_ratio() {
        // 48 kHz stereo device -> 44.1 kHz mono encoder, fed in WASAPI-like
        // 10 ms (480 frame) and ragged buffers.
        let mut conv = CaptureConverter::new(48_000, 2, 44_100, 1).unwrap();
        let mut produced = 0usize;
        let mut fed = 0usize;
        let sizes = [480usize, 441, 1000, 17, 480, 2048, 333];
        for i in 0..200 {
            let n = sizes[i % sizes.len()];
            let buf = vec![0.25f32; n * 2];
            produced += conv.process(&buf).len();
            fed += n;
        }
        let expected = fed as f64 * 44_100.0 / 48_000.0;
        // Everything but the buffered tail (< 2 chunks) must come out.
        assert!(produced as f64 > expected - 2.0 * CaptureConverter::CHUNK_FRAMES as f64);
        assert!((produced as f64) < expected + 64.0, "produced {produced}, expected ~{expected}");
    }

    #[test]
    fn converter_passthrough_maps_channels() {
        let mut down = CaptureConverter::new(44_100, 2, 44_100, 1).unwrap();
        assert_eq!(down.process(&[0.5, 0.0, -0.5, -0.5]), vec![8191, -16384]);

        let mut up = CaptureConverter::new(44_100, 1, 44_100, 2).unwrap();
        assert_eq!(up.process(&[1.0]), vec![32767, 32767]);

        // 4-channel interface downmixed to stereo keeps the first pair.
        let mut quad = CaptureConverter::new(44_100, 4, 44_100, 2).unwrap();
        assert_eq!(quad.process(&[1.0, -1.0, 0.0, 0.0]), vec![32767, -32768]);
    }

    #[test]
    fn test_channel_conversions() {
        let mono = vec![100, 200, 300];
        let stereo = mono_to_stereo_i16(&mono);
        assert_eq!(stereo, vec![100, 100, 200, 200, 300, 300]);

        let back_to_mono = stereo_to_mono_i16(&stereo);
        assert_eq!(back_to_mono, mono);
    }
}

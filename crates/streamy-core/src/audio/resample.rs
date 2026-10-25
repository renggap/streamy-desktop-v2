//! Audio sample format, channel layout, and sample-rate conversion (rubato).

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

pub struct AudioResampler {
    resampler: Option<SincFixedIn<f32>>,
    pub in_rate: u32,
    pub out_rate: u32,
    pub channels: usize,
}

impl AudioResampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize) -> Result<Self, String> {
        if in_rate == out_rate {
            return Ok(Self {
                resampler: None,
                in_rate,
                out_rate,
                channels,
            });
        }

        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };

        let chunk_size = 1024;
        let resampler = SincFixedIn::<f32>::new(
            out_rate as f64 / in_rate as f64,
            2.0,
            params,
            chunk_size,
            channels,
        )
        .map_err(|e| format!("Failed to create resampler: {:?}", e))?;

        Ok(Self {
            resampler: Some(resampler),
            in_rate,
            out_rate,
            channels,
        })
    }

    /// Resample interleaved f32 PCM if needed.
    pub fn process_interleaved_f32(&mut self, input: &[f32]) -> Result<Vec<f32>, String> {
        let resampler = match &mut self.resampler {
            Some(r) => r,
            None => return Ok(input.to_vec()),
        };

        let channels = self.channels;
        let frames = input.len() / channels;

        // De-interleave
        let mut deinterleaved = vec![Vec::with_capacity(frames); channels];
        for (i, &s) in input.iter().enumerate() {
            deinterleaved[i % channels].push(s);
        }

        let resampled = resampler
            .process(&deinterleaved, None)
            .map_err(|e| format!("Resampling error: {:?}", e))?;

        let out_frames = resampled[0].len();
        let mut out = Vec::with_capacity(out_frames * channels);
        for f in 0..out_frames {
            for c in 0..channels {
                out.push(resampled[c][f]);
            }
        }
        Ok(out)
    }
}

/// Convert f32 PCM to i16 PCM in-place or returning a new buffer.
pub fn f32_to_i16(input: &[f32]) -> Vec<i16> {
    input
        .iter()
        .map(|&s| {
            let mult = if s < 0.0 { 32768.0 } else { 32767.0 };
            (s * mult).clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// Convert i16 PCM to f32 PCM [-1.0, 1.0].
pub fn i16_to_f32(input: &[i16]) -> Vec<f32> {
    input
        .iter()
        .map(|&s| s as f32 / 32768.0)
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

        let roundtrip = i16_to_f32(&i);
        assert!((roundtrip[0] - (-1.0)).abs() < 0.001);
        assert!((roundtrip[2] - 0.5).abs() < 0.001);
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

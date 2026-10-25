//! Pure DSP helpers for the VU meter (pre-encoder level tap).
//! Operates on raw 16-bit PCM before reaching the encoder.

use serde::{Deserialize, Serialize};

pub const FLOOR_DB: f32 = -90.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelSnapshot {
    pub samples: Vec<f32>,
    pub rms_db: f32,
}

pub struct LevelTap;

impl LevelTap {
    /// Calculate RMS level in dBFS, floored at -90 dBFS.
    pub fn rms_db(pcm: &[i16]) -> f32 {
        if pcm.is_empty() {
            return FLOOR_DB;
        }
        let mut sum = 0.0;
        for &s in pcm {
            let n = s as f64 / 32768.0;
            sum += n * n;
        }
        let rms = (sum / pcm.len() as f64).sqrt();
        let db = (20.0 * (rms.max(1e-7)).log10()) as f32;
        if db < FLOOR_DB {
            FLOOR_DB
        } else {
            db
        }
    }

    /// Point-count waveform decimation to [-1.0, 1.0] floats for the scope view.
    pub fn decimate(pcm: &[i16], points: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; points];
        if pcm.is_empty() || points == 0 {
            return out;
        }
        let denom = (points - 1).max(1);
        for i in 0..points {
            let src = (i * (pcm.len() - 1)) / denom;
            out[i] = pcm[src] as f32 / 32768.0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_db_silence_is_floor_and_fullscale_is_zero() {
        let silence = vec![0i16; 1024];
        assert_eq!(LevelTap::rms_db(&silence), -90.0);

        let fullscale = vec![32767i16; 1024];
        let db = LevelTap::rms_db(&fullscale);
        assert!((db - 0.0).abs() < 0.01, "got {}", db);
    }

    #[test]
    fn rms_db_half_amplitude_is_minus_six() {
        let half = vec![-16384i16; 1024];
        let db = LevelTap::rms_db(&half);
        assert!((db - (-6.02)).abs() < 0.05, "got {}", db);
    }

    #[test]
    fn decimate_produces_normalized_fixed_length() {
        let mut pcm = vec![0i16; 1024];
        for i in 0..1024 {
            pcm[i] = (((i % 256) * 256) as i32 - 32768) as i16;
        }
        let out = LevelTap::decimate(&pcm, 96);
        assert_eq!(out.len(), 96);
        for &s in &out {
            assert!(s >= -1.0 && s <= 1.0, "sample out of range: {}", s);
        }
    }
}

//! Microphone noise suppression (RNNoise, via the pure-Rust `nnnoiseless` port).
//!
//! RNNoise only runs at 48 kHz in 480-frame (10 ms) blocks, so the mic is
//! converted to 48 kHz first, denoised per channel, then converted to the
//! encoder's format. The strength is a wet/dry blend: the dry path is delayed
//! by RNNoise's own 480-sample latency so the two never comb-filter, and the
//! blend ramps over a block so changing it live doesn't click.

use std::sync::Arc;
use nnnoiseless::DenoiseState;
use serde::{Deserialize, Serialize};

use crate::audio::resample::CaptureConverter;
use crate::audio::switch::InputControl;

const RATE: u32 = 48_000;
const BLOCK: usize = DenoiseState::FRAME_SIZE;
/// RNNoise works on 16-bit-range floats.
const SCALE: f32 = 32_768.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NoiseSuppression {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl NoiseSuppression {
    /// Share of the denoised signal; the rest is the original. The leftover
    /// noise sits about 10 dB (Low) / 20 dB (Medium) down; High removes all
    /// RNNoise can.
    pub fn wet(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Low => 0.7,
            Self::Medium => 0.9,
            Self::High => 1.0,
        }
    }

    pub(crate) fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Low,
            2 => Self::Medium,
            3 => Self::High,
            _ => Self::Off,
        }
    }
}

struct Lane {
    state: Option<Box<DenoiseState<'static>>>,
    input: Vec<f32>,
    /// Last block's input: the dry signal, aligned with RNNoise's output.
    prev: Vec<f32>,
    wet: Vec<f32>,
}

/// Device PCM -> (optionally denoised) PCM at the encoder's format.
pub struct MicProcessor {
    pre: CaptureConverter,
    post: CaptureConverter,
    control: Arc<InputControl>,
    lanes: Vec<Lane>,
    last_wet: f32,
    block: Vec<f32>,
}

impl MicProcessor {
    pub fn new(
        in_rate: u32,
        in_channels: usize,
        out_rate: u32,
        out_channels: usize,
        control: Arc<InputControl>,
    ) -> Result<Self, String> {
        Ok(Self {
            pre: CaptureConverter::new(in_rate, in_channels, RATE, out_channels)?,
            post: CaptureConverter::new(RATE, out_channels, out_rate, out_channels)?,
            control,
            lanes: (0..out_channels)
                .map(|_| Lane {
                    state: None,
                    input: Vec::with_capacity(BLOCK * 2),
                    prev: vec![0.0; BLOCK],
                    wet: vec![0.0; BLOCK],
                })
                .collect(),
            last_wet: 0.0,
            block: Vec::with_capacity(BLOCK * out_channels),
        })
    }

    /// Interleaved f32 at the device format in, interleaved i16 at the encoder format out.
    pub fn process(&mut self, input: &[f32]) -> Vec<i16> {
        let ch = self.lanes.len();
        for frame in self.pre.process_f32(input).chunks_exact(ch) {
            for (lane, &s) in self.lanes.iter_mut().zip(frame) {
                lane.input.push(s * SCALE);
            }
        }

        let mut out = Vec::new();
        while self.lanes[0].input.len() >= BLOCK {
            let target = self.control.noise_suppression().wet();
            let from = self.last_wet;
            self.block.clear();
            self.block.resize(BLOCK * ch, 0.0);
            let mut ramped = false;

            for (c, lane) in self.lanes.iter_mut().enumerate() {
                let x = &lane.input[..BLOCK];
                // A fresh model's first block fades in; keep it dry (wet stays 0).
                let warm = if target > 0.0 || from > 0.0 {
                    let warm = lane.state.is_some();
                    lane.state
                        .get_or_insert_with(DenoiseState::new)
                        .process_frame(&mut lane.wet, x);
                    warm
                } else {
                    lane.state = None; // off: skip the work, restart cleanly later
                    false
                };
                for i in 0..BLOCK {
                    let dry = lane.prev[i];
                    let w = if warm { from + (target - from) * (i as f32 / BLOCK as f32) } else { 0.0 };
                    self.block[i * ch + c] = (dry + (lane.wet[i] - dry) * w) / SCALE;
                }
                ramped = warm;
                lane.prev.copy_from_slice(x);
                lane.input.drain(..BLOCK);
            }

            self.last_wet = if ramped { target } else { 0.0 };
            out.extend(self.post.process(&self.block));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::switch::InputKind;

    /// Deterministic white noise in [-amp, amp].
    fn noise(n: usize, amp: f32, seed: &mut u32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((*seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    /// 48 kHz mono in and out (no resampling), so blocks map 1:1.
    fn run(level: NoiseSuppression, input: &[f32]) -> Vec<i16> {
        let control = InputControl::new(InputKind::Mic, 1.0);
        control.set_noise_suppression(level);
        let mut p = MicProcessor::new(48_000, 1, 48_000, 1, control).unwrap();
        input.chunks(BLOCK).flat_map(|b| p.process(b)).collect()
    }

    /// What RNNoise alone makes of `input`, in i16 units.
    fn rnnoise(input: &[f32]) -> Vec<f32> {
        let mut st = DenoiseState::new();
        let mut out = vec![0.0; input.len()];
        for at in (0..input.len() - BLOCK + 1).step_by(BLOCK) {
            let x: Vec<f32> = input[at..at + BLOCK].iter().map(|v| v * SCALE).collect();
            st.process_frame(&mut out[at..at + BLOCK], &x);
        }
        out
    }

    #[test]
    fn off_passes_audio_through_at_the_target_rate() {
        let control = InputControl::new(InputKind::Mic, 1.0);
        let mut p = MicProcessor::new(44_100, 2, 22_050, 1, control).unwrap();
        let mut produced = 0;
        for _ in 0..100 {
            produced += p.process(&vec![0.25; 441 * 2]).len();
        }
        // 1 s in -> ~1 s out, less what sits in the resamplers and the 10 ms block.
        assert!((21_000..=22_050).contains(&produced), "{produced}");
    }

    #[test]
    fn off_is_the_input_delayed_by_one_block() {
        let input = noise(BLOCK * 20, 0.05, &mut 1);
        let out = run(NoiseSuppression::Off, &input);
        assert_eq!(out.len(), input.len());
        assert!(out[..BLOCK].iter().all(|&s| s == 0));
        for (o, x) in out[BLOCK..].iter().zip(&input) {
            assert!((*o as f32 - x * SCALE).abs() <= 2.0, "{o} vs {}", x * SCALE);
        }
    }

    #[test]
    fn strength_blends_rnnoise_output_with_the_aligned_original() {
        let input = noise(BLOCK * 20, 0.05, &mut 2);
        let wet = rnnoise(&input);
        for level in [NoiseSuppression::Low, NoiseSuppression::Medium, NoiseSuppression::High] {
            let out = run(level, &input);
            let w = level.wet();
            // Block 0 is the model's dry warm-up, block 1 ramps from 0; from block 2 on it's steady.
            for i in BLOCK * 2..out.len() {
                let dry = input[i - BLOCK] * SCALE;
                let want = dry + (wet[i] - dry) * w;
                assert!((out[i] as f32 - want).abs() <= 2.0, "{level:?} sample {i}: {} vs {want}", out[i]);
            }
        }
    }

    #[test]
    fn switching_strength_live_keeps_the_stream_flowing() {
        let control = InputControl::new(InputKind::Mic, 1.0);
        let mut p = MicProcessor::new(48_000, 1, 48_000, 1, control.clone()).unwrap();
        let mut seed = 3;
        let mut total = 0;
        for (i, level) in [NoiseSuppression::Off, NoiseSuppression::High, NoiseSuppression::Off, NoiseSuppression::Medium]
            .iter()
            .cycle()
            .take(40)
            .enumerate()
        {
            control.set_noise_suppression(*level);
            let out = p.process(&noise(480, 0.1, &mut seed));
            assert!(i < 2 || !out.is_empty(), "block {i} produced nothing");
            total += out.len();
        }
        assert!(total >= 480 * 38);
    }
}

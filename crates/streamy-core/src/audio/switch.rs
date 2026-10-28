//! Live input switch between the microphone and the playlist player.
//!
//! Both inputs run for the whole session; only the selected one reaches the
//! pipeline, so switching is instant and never restarts the encoder or the
//! connection. When the mic is selected but silent (missing, unplugged), the
//! player's clock forwards silence so listeners stay connected.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::audio::player::{PlayerSource, PlaylistPlayer};
use crate::audio::source::{AudioSource, FrameCallback};

/// No mic frames for this long while the mic is selected: treat it as gone and
/// fill with silence.
const MIC_STALL: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    #[default]
    Mic,
    Playlist,
}

/// Shared, lock-free input settings read on the audio threads.
pub struct InputControl {
    kind: AtomicU8,
    mic_gain: AtomicU32,
    mic_failed: AtomicBool,
}

impl InputControl {
    pub fn new(kind: InputKind, mic_gain: f32) -> Arc<Self> {
        Arc::new(Self {
            kind: AtomicU8::new(kind as u8),
            mic_gain: AtomicU32::new(sanitize_gain(mic_gain).to_bits()),
            mic_failed: AtomicBool::new(false),
        })
    }

    pub fn kind(&self) -> InputKind {
        if self.kind.load(Ordering::Relaxed) == InputKind::Playlist as u8 {
            InputKind::Playlist
        } else {
            InputKind::Mic
        }
    }

    pub fn set_kind(&self, kind: InputKind) {
        self.kind.store(kind as u8, Ordering::Relaxed);
    }

    pub fn mic_gain(&self) -> f32 {
        f32::from_bits(self.mic_gain.load(Ordering::Relaxed))
    }

    pub fn set_mic_gain(&self, gain: f32) -> f32 {
        let g = sanitize_gain(gain);
        self.mic_gain.store(g.to_bits(), Ordering::Relaxed);
        g
    }

    /// True when the current session's mic could not start or stopped delivering.
    pub fn mic_failed(&self) -> bool {
        self.mic_failed.load(Ordering::Relaxed)
    }

    pub fn reset_mic_failed(&self) {
        self.mic_failed.store(false, Ordering::Relaxed);
    }
}

fn sanitize_gain(g: f32) -> f32 {
    if g.is_finite() {
        g.clamp(0.0, 4.0)
    } else {
        1.0
    }
}

struct Forward {
    on_frame: FrameCallback,
    alive: bool,
    last_kind: Option<InputKind>,
    mic_seen: Option<Instant>,
    scratch: Vec<i16>,
}

impl Forward {
    /// Forward `pcm` from `kind`, fading in over the first frame after a switch.
    fn send(&mut self, kind: InputKind, pcm: &[i16], gain: f32) -> bool {
        if !self.alive {
            return false;
        }
        let fade = self.last_kind != Some(kind);
        self.last_kind = Some(kind);
        let ok = if fade || gain != 1.0 {
            self.scratch.clear();
            let n = pcm.len().max(1) as f32;
            self.scratch.extend(pcm.iter().enumerate().map(|(i, &s)| {
                let g = if fade { gain * i as f32 / n } else { gain };
                (s as f32 * g).clamp(i16::MIN as f32, i16::MAX as f32) as i16
            }));
            (self.on_frame)(&self.scratch)
        } else {
            (self.on_frame)(pcm)
        };
        self.alive = ok;
        ok
    }
}

pub struct SwitchSource {
    mic: Box<dyn AudioSource>,
    player: PlayerSource,
    control: Arc<InputControl>,
}

impl SwitchSource {
    /// `mic` and the player view must produce the same rate and channel count.
    pub fn new(
        mic: Box<dyn AudioSource>,
        player: Arc<PlaylistPlayer>,
        sample_rate: u32,
        mono: bool,
        control: Arc<InputControl>,
    ) -> Self {
        Self {
            mic,
            player: PlayerSource::new(player, sample_rate, mono),
            control,
        }
    }
}

impl AudioSource for SwitchSource {
    fn start(&mut self, on_frame: FrameCallback) -> Result<(), String> {
        self.stop();
        self.control.reset_mic_failed();
        let fwd = Arc::new(Mutex::new(Forward {
            on_frame,
            alive: true,
            last_kind: None,
            mic_seen: None,
            scratch: Vec::new(),
        }));

        let (f, c) = (fwd.clone(), self.control.clone());
        self.player.start(Box::new(move |pcm| {
            let mut f = f.lock();
            match c.kind() {
                InputKind::Playlist => f.send(InputKind::Playlist, pcm, 1.0),
                InputKind::Mic => {
                    let stalled = f.mic_seen.is_none_or(|t| t.elapsed() > MIC_STALL);
                    if !stalled {
                        return f.alive;
                    }
                    c.mic_failed.store(true, Ordering::Relaxed);
                    // Fill with silence on the playlist clock so the stream stays up.
                    let silence = vec![0i16; pcm.len()];
                    f.send(InputKind::Mic, &silence, 1.0)
                }
            }
        }))?;

        let (f, c) = (fwd.clone(), self.control.clone());
        let mic_result = self.mic.start(Box::new(move |pcm| {
            let mut f = f.lock();
            f.mic_seen = Some(Instant::now());
            c.mic_failed.store(false, Ordering::Relaxed);
            match c.kind() {
                InputKind::Mic => f.send(InputKind::Mic, pcm, c.mic_gain()),
                InputKind::Playlist => f.alive,
            }
        }));
        if let Err(e) = mic_result {
            if self.control.kind() == InputKind::Mic {
                self.player.stop();
                return Err(e);
            }
            tracing::warn!("mic unavailable, continuing with playlist only: {e}");
            self.control.mic_failed.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.mic.stop();
        self.player.stop();
    }
}

impl Drop for SwitchSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::source::SineAudioSource;
    use std::thread;

    struct FailingSource;
    impl AudioSource for FailingSource {
        fn start(&mut self, _: FrameCallback) -> Result<(), String> {
            Err("mic_not_found".into())
        }
        fn stop(&mut self) {}
    }

    type Harness = (SwitchSource, Arc<InputControl>, Arc<Mutex<Vec<i16>>>, Arc<PlaylistPlayer>);

    fn collect(kind: InputKind, mic: Box<dyn AudioSource>) -> Harness {
        let player = PlaylistPlayer::new();
        let control = InputControl::new(kind, 1.0);
        let got = Arc::new(Mutex::new(Vec::new()));
        let mut src = SwitchSource::new(mic, player.clone(), 22_050, true, control.clone());
        let g = got.clone();
        src.start(Box::new(move |pcm| {
            g.lock().extend_from_slice(pcm);
            true
        }))
        .unwrap();
        (src, control, got, player)
    }

    fn peak(v: &[i16]) -> i32 {
        v.iter().map(|s| (*s as i32).abs()).max().unwrap_or(0)
    }

    #[test]
    fn routes_only_the_selected_input() {
        let (mut src, control, got, player) = collect(InputKind::Mic, Box::new(SineAudioSource::new(22_050, true, 1024)));
        thread::sleep(Duration::from_millis(300));
        assert!(peak(&got.lock()) > 8_000, "mic tone reaches the pipeline");

        control.set_kind(InputKind::Playlist);
        thread::sleep(Duration::from_millis(150));
        got.lock().clear();
        thread::sleep(Duration::from_millis(300));
        let after = got.lock().clone();
        assert!(!after.is_empty(), "the playlist clock keeps frames flowing");
        assert_eq!(peak(&after), 0, "stopped playlist is silent, mic is muted");

        control.set_mic_gain(0.0);
        control.set_kind(InputKind::Mic);
        thread::sleep(Duration::from_millis(150));
        got.lock().clear();
        thread::sleep(Duration::from_millis(200));
        assert_eq!(peak(&got.lock()), 0, "mic gain applies");
        assert!(!control.mic_failed());
        src.stop();
        player.shutdown();
    }

    #[test]
    fn missing_mic_fails_on_mic_and_falls_back_on_playlist() {
        let player = PlaylistPlayer::new();
        let control = InputControl::new(InputKind::Mic, 1.0);
        let mut src = SwitchSource::new(Box::new(FailingSource), player.clone(), 22_050, true, control.clone());
        assert!(src.start(Box::new(|_| true)).is_err());

        control.set_kind(InputKind::Playlist);
        let got = Arc::new(Mutex::new(0usize));
        let g = got.clone();
        src.start(Box::new(move |pcm| {
            *g.lock() += pcm.len();
            true
        }))
        .unwrap();
        assert!(control.mic_failed());
        thread::sleep(Duration::from_millis(250));
        assert!(*got.lock() > 0);

        // Switching to the dead mic keeps the stream alive with silence.
        control.set_kind(InputKind::Mic);
        *got.lock() = 0;
        thread::sleep(Duration::from_millis(250));
        assert!(*got.lock() > 0);
        src.stop();
        player.shutdown();
    }
}

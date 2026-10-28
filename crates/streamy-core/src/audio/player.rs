//! Playlist player: plays local files in real time and fans the audio out to
//! taps (the broadcast/record input, local monitoring).
//!
//! A clock thread renders 20 ms blocks at `PLAYER_RATE` stereo. While stopped
//! or paused it renders silence, so a broadcast fed by the player keeps a
//! steady stream instead of starving the server. With no taps and nothing
//! playing the thread idles.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use serde::Serialize;

use crate::audio::decoder::{probe_file, FileDecoder, PLAYER_CHANNELS, PLAYER_RATE};
use crate::audio::resample::CaptureConverter;
use crate::audio::source::{AudioSource, FrameCallback};

pub const BLOCK_FRAMES: usize = 960;
const BLOCK: Duration = Duration::from_millis(20);
const IDLE_POLL: Duration = Duration::from_millis(40);
/// Behind schedule by more than this (a slow disk, a suspended laptop): resync
/// the clock instead of bursting to catch up.
const MAX_LAG: Duration = Duration::from_millis(200);
/// "Previous" within this many seconds of a track's start goes to the previous
/// track; later, it restarts the current one.
const RESTART_THRESHOLD_SECS: f64 = 3.0;
/// Frames handed to an `AudioSource` consumer per call, like the mic capture.
const EMIT_FRAMES: usize = 1024;

/// Receives each rendered block: interleaved f32, `PLAYER_RATE`, `PLAYER_CHANNELS`.
pub type PlayerTap = Box<dyn FnMut(&[f32]) + Send>;
pub type PlayerEventFn = Arc<dyn Fn(PlayerEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Track {
    pub id: u64,
    pub path: String,
    pub title: String,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerStatus {
    pub state: PlayState,
    pub current_id: Option<u64>,
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub volume: f32,
    pub auto_advance: bool,
    pub repeat: bool,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// State, current track, queue or options changed.
    Changed,
    /// A track began playing (manually or by auto-advance).
    TrackStarted(Track),
    /// A track could not be played and was skipped.
    TrackFailed { track: Track, reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct AddResult {
    pub added: Vec<Track>,
    /// (path, reason) for files that could not be read.
    pub failed: Vec<(String, String)>,
}

struct Inner {
    queue: Vec<Track>,
    current: Option<u64>,
    state: PlayState,
    decoder: Option<FileDecoder>,
    volume: f32,
    auto_advance: bool,
    repeat: bool,
    next_id: u64,
}

impl Inner {
    fn index_of(&self, id: u64) -> Option<usize> {
        self.queue.iter().position(|t| t.id == id)
    }
}

pub struct PlaylistPlayer {
    inner: Mutex<Inner>,
    taps: Mutex<Vec<(u64, PlayerTap)>>,
    next_tap: AtomicU64,
    on_event: Mutex<Option<PlayerEventFn>>,
    running: AtomicBool,
    clock: Mutex<Option<thread::JoinHandle<()>>>,
}

impl PlaylistPlayer {
    pub fn new() -> Arc<Self> {
        let player = Arc::new(Self {
            inner: Mutex::new(Inner {
                queue: Vec::new(),
                current: None,
                state: PlayState::Stopped,
                decoder: None,
                volume: 1.0,
                auto_advance: true,
                repeat: false,
                next_id: 1,
            }),
            taps: Mutex::new(Vec::new()),
            next_tap: AtomicU64::new(1),
            on_event: Mutex::new(None),
            running: AtomicBool::new(true),
            clock: Mutex::new(None),
        });
        let weak = Arc::downgrade(&player);
        let handle = thread::Builder::new()
            .name("streamy-player".into())
            .spawn(move || clock_loop(weak))
            .expect("failed to spawn player thread");
        *player.clock.lock() = Some(handle);
        player
    }

    pub fn set_event_handler(&self, f: PlayerEventFn) {
        *self.on_event.lock() = Some(f);
    }

    fn fire(&self, events: Vec<PlayerEvent>) {
        if events.is_empty() {
            return;
        }
        let handler = self.on_event.lock().clone();
        if let Some(h) = handler {
            for e in events {
                h(e);
            }
        }
    }

    // ---- queue --------------------------------------------------------------

    pub fn tracks(&self) -> Vec<Track> {
        self.inner.lock().queue.clone()
    }

    /// Probe and append files. Unreadable files are reported, not added.
    pub fn add_files(&self, paths: &[String]) -> AddResult {
        let mut probed = Vec::new();
        let mut failed = Vec::new();
        for p in paths {
            match probe_file(Path::new(p)) {
                Ok(meta) => probed.push((p.clone(), meta)),
                Err(e) => failed.push((p.clone(), e)),
            }
        }
        let added: Vec<Track> = {
            let mut inner = self.inner.lock();
            probed
                .into_iter()
                .map(|(path, meta)| {
                    let t = Track {
                        id: inner.next_id,
                        path,
                        title: meta.title,
                        duration_secs: meta.duration_secs,
                    };
                    inner.next_id += 1;
                    inner.queue.push(t.clone());
                    t
                })
                .collect()
        };
        if !added.is_empty() {
            self.fire(vec![PlayerEvent::Changed]);
        }
        AddResult { added, failed }
    }

    pub fn remove(&self, id: u64) {
        {
            let mut inner = self.inner.lock();
            let Some(i) = inner.index_of(id) else { return };
            inner.queue.remove(i);
            if inner.current == Some(id) {
                inner.current = None;
                inner.decoder = None;
                inner.state = PlayState::Stopped;
            }
        }
        self.fire(vec![PlayerEvent::Changed]);
    }

    /// Move a track to `to` (clamped), shifting the others.
    pub fn move_track(&self, id: u64, to: usize) {
        {
            let mut inner = self.inner.lock();
            let Some(from) = inner.index_of(id) else { return };
            let t = inner.queue.remove(from);
            let to = to.min(inner.queue.len());
            inner.queue.insert(to, t);
        }
        self.fire(vec![PlayerEvent::Changed]);
    }

    pub fn clear(&self) {
        {
            let mut inner = self.inner.lock();
            inner.queue.clear();
            inner.current = None;
            inner.decoder = None;
            inner.state = PlayState::Stopped;
        }
        self.fire(vec![PlayerEvent::Changed]);
    }

    // ---- transport ----------------------------------------------------------

    pub fn status(&self) -> PlayerStatus {
        let inner = self.inner.lock();
        let current = inner.current.and_then(|id| inner.queue.iter().find(|t| t.id == id));
        PlayerStatus {
            state: inner.state,
            current_id: inner.current,
            position_secs: inner.decoder.as_ref().map(|d| d.position_secs()).unwrap_or(0.0),
            duration_secs: inner
                .decoder
                .as_ref()
                .and_then(|d| d.duration_secs())
                .or_else(|| current.and_then(|t| t.duration_secs)),
            volume: inner.volume,
            auto_advance: inner.auto_advance,
            repeat: inner.repeat,
        }
    }

    pub fn current_track(&self) -> Option<Track> {
        let inner = self.inner.lock();
        inner.current.and_then(|id| inner.queue.iter().find(|t| t.id == id).cloned())
    }

    /// Play `id`, or resume / start the current (else first) track.
    pub fn play(&self, id: Option<u64>) -> Result<(), String> {
        let target = {
            let mut inner = self.inner.lock();
            match id {
                Some(id) => id,
                None => {
                    if inner.state == PlayState::Paused && inner.decoder.is_some() {
                        inner.state = PlayState::Playing;
                        drop(inner);
                        self.fire(vec![PlayerEvent::Changed]);
                        return Ok(());
                    }
                    if inner.state == PlayState::Playing {
                        return Ok(());
                    }
                    match inner.current.filter(|c| inner.index_of(*c).is_some()).or(inner.queue.first().map(|t| t.id)) {
                        Some(id) => id,
                        None => return Err("playlist_empty".into()),
                    }
                }
            }
        };
        let track = self
            .inner
            .lock()
            .queue
            .iter()
            .find(|t| t.id == target)
            .cloned()
            .ok_or_else(|| "track_missing".to_string())?;
        // Open outside the lock: the clock thread must keep rendering meanwhile.
        let decoder = FileDecoder::open(Path::new(&track.path))?;
        {
            let mut inner = self.inner.lock();
            if inner.index_of(track.id).is_none() {
                return Err("track_missing".into()); // removed while opening
            }
            inner.decoder = Some(decoder);
            inner.current = Some(track.id);
            inner.state = PlayState::Playing;
        }
        self.fire(vec![PlayerEvent::TrackStarted(track), PlayerEvent::Changed]);
        Ok(())
    }

    pub fn pause(&self) {
        let changed = {
            let mut inner = self.inner.lock();
            if inner.state == PlayState::Playing {
                inner.state = PlayState::Paused;
                true
            } else {
                false
            }
        };
        if changed {
            self.fire(vec![PlayerEvent::Changed]);
        }
    }

    pub fn stop(&self) {
        let changed = {
            let mut inner = self.inner.lock();
            let changed = inner.state != PlayState::Stopped || inner.decoder.is_some();
            inner.state = PlayState::Stopped;
            inner.decoder = None;
            changed
        };
        if changed {
            self.fire(vec![PlayerEvent::Changed]);
        }
    }

    pub fn next(&self) -> Result<(), String> {
        let target = {
            let inner = self.inner.lock();
            let len = inner.queue.len();
            if len == 0 {
                return Err("playlist_empty".into());
            }
            let next = match inner.current.and_then(|c| inner.index_of(c)) {
                Some(i) if i + 1 < len => i + 1,
                Some(_) => 0,
                None => 0,
            };
            inner.queue[next].id
        };
        self.play(Some(target))
    }

    pub fn previous(&self) -> Result<(), String> {
        let (target, restart) = {
            let inner = self.inner.lock();
            if inner.queue.is_empty() {
                return Err("playlist_empty".into());
            }
            let pos = inner.decoder.as_ref().map(|d| d.position_secs()).unwrap_or(0.0);
            match inner.current.and_then(|c| inner.index_of(c)) {
                Some(i) if pos > RESTART_THRESHOLD_SECS || i == 0 => (inner.queue[i].id, true),
                Some(i) => (inner.queue[i - 1].id, false),
                None => (inner.queue[0].id, false),
            }
        };
        if restart && self.inner.lock().decoder.is_some() {
            return self.seek(0.0);
        }
        self.play(Some(target))
    }

    pub fn seek(&self, secs: f64) -> Result<(), String> {
        {
            let mut inner = self.inner.lock();
            let Some(dec) = inner.decoder.as_mut() else {
                return Err("nothing_playing".into());
            };
            dec.seek(secs)?;
        }
        self.fire(vec![PlayerEvent::Changed]);
        Ok(())
    }

    pub fn set_volume(&self, volume: f32) -> f32 {
        let v = if volume.is_finite() { volume.clamp(0.0, 2.0) } else { 1.0 };
        self.inner.lock().volume = v;
        v
    }

    pub fn set_options(&self, auto_advance: bool, repeat: bool) {
        {
            let mut inner = self.inner.lock();
            inner.auto_advance = auto_advance;
            inner.repeat = repeat;
        }
        self.fire(vec![PlayerEvent::Changed]);
    }

    // ---- taps ---------------------------------------------------------------

    pub fn add_tap(&self, tap: PlayerTap) -> u64 {
        let id = self.next_tap.fetch_add(1, Ordering::SeqCst);
        self.taps.lock().push((id, tap));
        id
    }

    /// Detach a tap. Blocks until a block being delivered to it has finished,
    /// so it must not be called from inside a tap.
    pub fn remove_tap(&self, id: u64) {
        self.taps.lock().retain(|(t, _)| *t != id);
    }

    pub fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        let handle = self.clock.lock().take();
        if let Some(h) = handle {
            if h.thread().id() != thread::current().id() {
                let _ = h.join();
            }
        }
        self.taps.lock().clear();
        let mut inner = self.inner.lock();
        inner.decoder = None;
        inner.state = PlayState::Stopped;
    }

    // ---- rendering (clock thread) -------------------------------------------

    fn is_idle(&self) -> bool {
        self.taps.lock().is_empty() && self.inner.lock().state != PlayState::Playing
    }

    fn render_block(&self, out: &mut [f32], events: &mut Vec<PlayerEvent>) {
        out.fill(0.0);
        let mut inner = self.inner.lock();
        if inner.state != PlayState::Playing {
            return;
        }
        let mut filled = 0;
        // Bounded: each pass either fills the block or moves to another track.
        for _ in 0..=inner.queue.len().min(8) {
            let Some(dec) = inner.decoder.as_mut() else {
                inner.state = PlayState::Stopped;
                events.push(PlayerEvent::Changed);
                break;
            };
            filled += dec.read(&mut out[filled * PLAYER_CHANNELS..]);
            if filled >= BLOCK_FRAMES {
                break;
            }
            self.advance_locked(&mut inner, events);
            if inner.state != PlayState::Playing {
                break;
            }
        }
        let vol = inner.volume;
        if vol != 1.0 {
            for s in out[..filled * PLAYER_CHANNELS].iter_mut() {
                *s *= vol;
            }
        }
    }

    /// The current track ended: open the next one or stop.
    fn advance_locked(&self, inner: &mut Inner, events: &mut Vec<PlayerEvent>) {
        inner.decoder = None;
        let len = inner.queue.len();
        let idx = inner.current.and_then(|c| inner.index_of(c));
        if !inner.auto_advance || len == 0 {
            inner.state = PlayState::Stopped;
            events.push(PlayerEvent::Changed);
            return;
        }
        let mut next = match idx {
            Some(i) => i + 1,
            None => 0,
        };
        for _ in 0..len {
            if next >= len {
                if inner.repeat {
                    next = 0;
                } else {
                    break;
                }
            }
            let track = inner.queue[next].clone();
            match FileDecoder::open(Path::new(&track.path)) {
                Ok(d) => {
                    inner.decoder = Some(d);
                    inner.current = Some(track.id);
                    events.push(PlayerEvent::TrackStarted(track));
                    events.push(PlayerEvent::Changed);
                    return;
                }
                Err(reason) => {
                    tracing::warn!("playlist: skipping {:?}: {reason}", track.path);
                    events.push(PlayerEvent::TrackFailed { track, reason });
                    next += 1;
                }
            }
        }
        inner.state = PlayState::Stopped;
        events.push(PlayerEvent::Changed);
    }
}

fn clock_loop(weak: Weak<PlaylistPlayer>) {
    let mut block = vec![0.0f32; BLOCK_FRAMES * PLAYER_CHANNELS];
    let mut deadline = Instant::now();
    loop {
        let Some(player) = weak.upgrade() else { return };
        if !player.running.load(Ordering::SeqCst) {
            return;
        }
        if player.is_idle() {
            drop(player);
            thread::sleep(IDLE_POLL);
            deadline = Instant::now();
            continue;
        }

        let mut events = Vec::new();
        player.render_block(&mut block, &mut events);
        for (_, tap) in player.taps.lock().iter_mut() {
            tap(&block);
        }
        player.fire(events);
        drop(player);

        deadline += BLOCK;
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        } else if now - deadline > MAX_LAG {
            deadline = now;
        }
    }
}

impl Drop for PlaylistPlayer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// `AudioSource` view of the player at an encoder's format, for the pipeline.
pub struct PlayerSource {
    player: Arc<PlaylistPlayer>,
    sample_rate: u32,
    mono: bool,
    tap: Option<u64>,
}

impl PlayerSource {
    pub fn new(player: Arc<PlaylistPlayer>, sample_rate: u32, mono: bool) -> Self {
        Self {
            player,
            sample_rate,
            mono,
            tap: None,
        }
    }
}

impl AudioSource for PlayerSource {
    fn start(&mut self, mut on_frame: FrameCallback) -> Result<(), String> {
        self.stop();
        let out_channels = if self.mono { 1 } else { 2 };
        let mut converter = CaptureConverter::new(PLAYER_RATE, PLAYER_CHANNELS, self.sample_rate, out_channels)?;
        let slice = EMIT_FRAMES * out_channels;
        let mut buffer: Vec<i16> = Vec::with_capacity(slice * 2);
        let mut alive = true;
        self.tap = Some(self.player.add_tap(Box::new(move |block| {
            if !alive {
                return;
            }
            buffer.extend(converter.process(block));
            while buffer.len() >= slice {
                if !on_frame(&buffer[..slice]) {
                    alive = false;
                    return;
                }
                buffer.drain(..slice);
            }
        })));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(id) = self.tap.take() {
            self.player.remove_tap(id);
        }
    }
}

impl Drop for PlayerSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decoder::tests::write_wav;

    fn wait_for(cond: impl Fn() -> bool, ms: u64) -> bool {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if cond() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        cond()
    }

    #[test]
    fn plays_through_the_queue_and_stops_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("one.wav");
        let b = dir.path().join("two.wav");
        write_wav(&a, 44_100, 1, 0.3);
        write_wav(&b, 22_050, 2, 0.3);

        let player = PlaylistPlayer::new();
        let started = Arc::new(Mutex::new(Vec::<String>::new()));
        let s = started.clone();
        player.set_event_handler(Arc::new(move |e| {
            if let PlayerEvent::TrackStarted(t) = e {
                s.lock().push(t.title);
            }
        }));
        let res = player.add_files(&[
            a.to_string_lossy().into(),
            "/no/such/file.mp3".into(),
            b.to_string_lossy().into(),
        ]);
        assert_eq!(res.added.len(), 2);
        assert_eq!(res.failed.len(), 1);

        player.play(None).unwrap();
        assert_eq!(player.status().state, PlayState::Playing);
        assert!(wait_for(|| player.status().state == PlayState::Stopped, 3_000));
        assert_eq!(*started.lock(), vec!["one".to_string(), "two".to_string()]);
        player.shutdown();
    }

    #[test]
    fn taps_get_silence_while_stopped_and_audio_while_playing() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("tone.wav");
        write_wav(&a, 48_000, 2, 1.0);
        let player = PlaylistPlayer::new();
        player.add_files(&[a.to_string_lossy().into()]);

        let peak = Arc::new(Mutex::new((0usize, 0.0f32)));
        let p = peak.clone();
        let tap = player.add_tap(Box::new(move |block| {
            let mut g = p.lock();
            g.0 += 1;
            g.1 = g.1.max(block.iter().fold(0.0f32, |m, s| m.max(s.abs())));
        }));
        assert!(wait_for(|| peak.lock().0 >= 5, 1_000), "a tap keeps the clock running");
        assert_eq!(peak.lock().1, 0.0, "stopped player renders silence");

        player.play(None).unwrap();
        assert!(wait_for(|| peak.lock().1 > 0.1, 1_000));
        player.remove_tap(tap);
        player.shutdown();
    }

    #[test]
    fn player_source_delivers_encoder_format_frames() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("tone.wav");
        write_wav(&a, 44_100, 2, 1.0);
        let player = PlaylistPlayer::new();
        player.add_files(&[a.to_string_lossy().into()]);
        player.play(None).unwrap();

        let got = Arc::new(Mutex::new(Vec::<usize>::new()));
        let g = got.clone();
        let mut src = PlayerSource::new(player.clone(), 22_050, true);
        src.start(Box::new(move |pcm| {
            g.lock().push(pcm.len());
            true
        }))
        .unwrap();
        assert!(wait_for(|| got.lock().len() >= 3, 1_500));
        src.stop();
        assert!(got.lock().iter().all(|&n| n == EMIT_FRAMES));
        player.shutdown();
    }

    #[test]
    fn previous_restarts_after_a_few_seconds_and_queue_edits_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wav");
        let b = dir.path().join("b.wav");
        write_wav(&a, 48_000, 1, 6.0);
        write_wav(&b, 48_000, 1, 6.0);
        let player = PlaylistPlayer::new();
        let ids: Vec<u64> = player
            .add_files(&[a.to_string_lossy().into(), b.to_string_lossy().into()])
            .added
            .iter()
            .map(|t| t.id)
            .collect();

        player.play(Some(ids[1])).unwrap();
        player.seek(4.0).unwrap();
        player.previous().unwrap();
        assert_eq!(player.status().current_id, Some(ids[1]));
        assert!(player.status().position_secs < 1.0);
        player.previous().unwrap();
        assert_eq!(player.status().current_id, Some(ids[0]));

        player.move_track(ids[0], 5);
        assert_eq!(player.tracks().iter().map(|t| t.id).collect::<Vec<_>>(), vec![ids[1], ids[0]]);
        player.remove(ids[0]);
        assert_eq!(player.status().state, PlayState::Stopped);
        assert_eq!(player.status().current_id, None);
        player.shutdown();
    }
}

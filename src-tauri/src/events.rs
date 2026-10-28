//! Event names emitted from Rust core to Svelte UI (keep in sync with src/lib/api.ts).

use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use streamy_core::audio::pipeline::LevelFeedFn;
use streamy_core::core::level::LevelSnapshot;

pub const EVENT_STREAM_STATE: &str = "stream-state-changed";
pub const EVENT_LEVELS: &str = "levels-changed";
pub const EVENT_RECORD_STATE: &str = "record-state-changed";
pub const EVENT_STREAM_RECORDING: &str = "stream-recording-changed";
pub const EVENT_PLAYER_CHANGED: &str = "player-changed";
pub const EVENT_PLAYER_ERROR: &str = "player-error";
pub const EVENT_UPDATE_CHANGED: &str = "update-changed";

#[derive(Debug, Clone, Serialize)]
pub struct PlayerErrorPayload {
    pub title: String,
    pub reason: String,
}

/// ~30 Hz is plenty for a meter; the pipeline produces ~43 snapshots/s and IPC isn't free.
const LEVEL_EMIT_INTERVAL: Duration = Duration::from_millis(33);
const SILENCE_DB: f32 = -90.0;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LevelPayload {
    pub rms_db: f32,
    pub peak_db: f32,
}

struct LevelWindow {
    last_emit: Instant,
    rms_db: f32,
    peak_db: f32,
}

/// Level feed that coalesces snapshots (max over the window) and emits at ~30 Hz.
pub fn throttled_level_feed(handle: AppHandle) -> LevelFeedFn {
    let window = Mutex::new(LevelWindow {
        last_emit: Instant::now(),
        rms_db: SILENCE_DB,
        peak_db: SILENCE_DB,
    });
    Arc::new(move |snap: LevelSnapshot| {
        let payload = {
            let mut w = window.lock();
            w.rms_db = w.rms_db.max(snap.rms_db);
            w.peak_db = w.peak_db.max(snap.peak_db);
            if w.last_emit.elapsed() < LEVEL_EMIT_INTERVAL {
                return;
            }
            let p = LevelPayload { rms_db: w.rms_db, peak_db: w.peak_db };
            w.last_emit = Instant::now();
            w.rms_db = SILENCE_DB;
            w.peak_db = SILENCE_DB;
            p
        };
        let _ = handle.emit(EVENT_LEVELS, payload);
    })
}

/// Tell the meter to fall back to silence once capture stops.
pub fn emit_silence(handle: &AppHandle) {
    let _ = handle.emit(
        EVENT_LEVELS,
        LevelPayload { rms_db: SILENCE_DB, peak_db: SILENCE_DB },
    );
}

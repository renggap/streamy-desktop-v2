//! Power management and sleep inhibitor.
//! Windows equivalent of Android WakeLock + WifiLock:
//! SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) while broadcasting or recording.
//!
//! `SetThreadExecutionState` is per-thread: a request made from a Tauri/blocking
//! pool thread is dropped when that thread exits, and a release from another
//! thread never clears it. So one dedicated thread owns the execution state,
//! and activities hold independent flags instead of a refcount that could drift.

use std::sync::mpsc;
use parking_lot::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Stream = 0b01,
    Record = 0b10,
}

pub struct PowerInhibitor {
    mask: Mutex<u8>,
    tx: Mutex<Option<mpsc::Sender<bool>>>,
}

impl PowerInhibitor {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<bool>();
        let spawned = std::thread::Builder::new()
            .name("streamy-power".into())
            .spawn(move || {
                for inhibit in rx {
                    apply(inhibit);
                }
                apply(false);
            })
            .is_ok();
        Self {
            mask: Mutex::new(0),
            tx: Mutex::new(spawned.then_some(tx)),
        }
    }

    /// True while any activity keeps the system awake.
    pub fn is_held(&self) -> bool {
        *self.mask.lock() != 0
    }

    /// Set or clear one activity's hold. Idempotent per activity.
    pub fn set(&self, hold: Hold, on: bool) {
        let mut mask = self.mask.lock();
        let before = *mask != 0;
        if on {
            *mask |= hold as u8;
        } else {
            *mask &= !(hold as u8);
        }
        let after = *mask != 0;
        if before != after {
            if let Some(tx) = self.tx.lock().as_ref() {
                let _ = tx.send(after);
            }
        }
    }
}

impl Default for PowerInhibitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PowerInhibitor {
    fn drop(&mut self) {
        // Closing the channel ends the thread, which clears the request on exit.
        self.tx.lock().take();
    }
}

#[cfg(windows)]
fn apply(inhibit: bool) {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };
    unsafe {
        if inhibit {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        } else {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(not(windows))]
fn apply(inhibit: bool) {
    tracing::debug!("sleep inhibit: {inhibit}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_holds_keep_inhibitor_held() {
        let p = PowerInhibitor::new();
        p.set(Hold::Stream, true);
        p.set(Hold::Record, true);
        p.set(Hold::Record, false); // stop record -> still held
        assert!(p.is_held());
        p.set(Hold::Stream, false); // stop stream -> released
        assert!(!p.is_held());
    }

    #[test]
    fn repeated_set_and_clear_are_idempotent() {
        let p = PowerInhibitor::new();
        p.set(Hold::Stream, false);
        p.set(Hold::Stream, true);
        p.set(Hold::Stream, true);
        p.set(Hold::Stream, false);
        assert!(!p.is_held());
    }
}

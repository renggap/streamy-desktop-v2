//! Power management and sleep inhibitor.
//! Windows equivalent of Android WakeLock + WifiLock:
//! SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) while broadcasting or recording.
//! Refcounted: stream and record sessions can hold it independently.

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct PowerInhibitor {
    active: AtomicUsize,
}

impl PowerInhibitor {
    pub const fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
        }
    }

    /// Current hold count (0 = sleep allowed). Test/monitoring hook.
    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// Inhibit system sleep while broadcasting or recording.
    pub fn acquire(&self) {
        if self.active.fetch_add(1, Ordering::SeqCst) == 0 {
            #[cfg(windows)]
            unsafe {
                use windows_sys::Win32::System::Power::{
                    SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
                };
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
            }
        }
    }

    /// Release one hold on sleep inhibition. Only the last release clears it.
    /// `compare_exchange` loop instead of `fetch_sub` + `store(0)`: a plain store
    /// after an underflowing sub can race a concurrent acquire and wipe real holds.
    pub fn release(&self) {
        let mut cur = self.active.load(Ordering::SeqCst);
        loop {
            if cur == 0 {
                return; // nothing held: ignore extra release
            }
            let next = cur - 1;
            match self
                .active
                .compare_exchange(cur, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => {
                    if next == 0 {
                        #[cfg(windows)]
                        unsafe {
                            use windows_sys::Win32::System::Power::{
                                SetThreadExecutionState, ES_CONTINUOUS,
                            };
                            SetThreadExecutionState(ES_CONTINUOUS);
                        }
                    }
                    return;
                }
                Err(actual) => cur = actual,
            }
        }
    }
}

impl Drop for PowerInhibitor {
    fn drop(&mut self) {
        self.active.store(0, Ordering::SeqCst);
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_acquire_release_keeps_inhibitor_held() {
        let p = PowerInhibitor::new();
        p.acquire(); // stream
        p.acquire(); // record
        p.release(); // stop record -> still held
        assert_eq!(p.active.load(Ordering::SeqCst), 1);
        p.release(); // stop stream -> released
        assert_eq!(p.active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn extra_release_does_not_underflow() {
        let p = PowerInhibitor::new();
        p.release();
        p.release();
        assert_eq!(p.active.load(Ordering::SeqCst), 0);
        p.acquire();
        assert_eq!(p.active.load(Ordering::SeqCst), 1);
    }
}

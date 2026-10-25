//! Power management and sleep inhibitor.
//! Windows equivalent of Android WakeLock + WifiLock:
//! SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) while broadcasting or recording.

use std::sync::atomic::{AtomicBool, Ordering};

pub struct PowerInhibitor {
    active: AtomicBool,
}

impl PowerInhibitor {
    pub const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
        }
    }

    /// Inhibit system sleep while broadcasting or recording.
    pub fn acquire(&self) {
        if self.active.swap(true, Ordering::SeqCst) {
            return;
        }

        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Power::{
                SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
            };
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        }
    }

    /// Release sleep inhibition on teardown.
    pub fn release(&self) {
        if !self.active.swap(false, Ordering::SeqCst) {
            return;
        }

        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

impl Drop for PowerInhibitor {
    fn drop(&mut self) {
        self.release();
    }
}

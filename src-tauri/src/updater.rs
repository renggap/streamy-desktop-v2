//! Silent auto-update from GitHub Releases (tauri-plugin-updater).
//!
//! A background thread checks `latest.json` on the latest release shortly
//! after launch and every few hours, downloads a newer signed installer in the
//! background, and installs it only while the app is idle: never on air,
//! recording, testing the mic or playing the playlist. The installer runs
//! quietly and relaunches the app.

use std::time::Duration;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::commands::AppState;
use crate::events::EVENT_UPDATE_CHANGED;

const FIRST_CHECK: Duration = Duration::from_secs(10);
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(60 * 60);
const IDLE_POLL: Duration = Duration::from_secs(20);
/// Let the UI show "Installing" before the app goes away.
const INSTALL_GRACE: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum UpdateStatus {
    #[default]
    Idle,
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    /// Downloaded; waits for the app to be idle.
    Ready { version: String },
    Installing { version: String },
}

#[derive(Default)]
pub struct UpdateTracker {
    status: Mutex<UpdateStatus>,
}

impl UpdateTracker {
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().clone()
    }
}

fn set_status(app: &AppHandle, status: UpdateStatus) {
    let tracker = app.state::<UpdateTracker>();
    let mut cur = tracker.status.lock();
    if *cur != status {
        *cur = status.clone();
        drop(cur);
        let _ = app.emit(EVENT_UPDATE_CHANGED, status);
    }
}

/// Start the background updater. Release builds only: a dev build would
/// otherwise "update" itself to the published installer.
pub fn spawn(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("streamy-updater".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                let wait = match tauri::async_runtime::block_on(fetch(&app)) {
                    Ok(Some((update, bytes))) => {
                        install_when_idle(&app, update, bytes);
                        return; // install ends the process; nothing to loop for
                    }
                    Ok(None) => RECHECK,
                    Err(e) => {
                        tracing::warn!("update check failed: {e}");
                        set_status(&app, UpdateStatus::Idle);
                        RETRY_AFTER_ERROR
                    }
                };
                std::thread::sleep(wait);
            }
        });
}

/// Check for a newer release and download it.
async fn fetch(app: &AppHandle) -> Result<Option<(Update, Vec<u8>)>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let version = update.version.clone();
    tracing::info!("downloading update {version}");
    set_status(app, UpdateStatus::Downloading { version: version.clone(), percent: Some(0) });

    let mut received: u64 = 0;
    let bytes = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                let percent = total.filter(|t| *t > 0).map(|t| (received * 100 / t).min(100) as u8);
                set_status(app, UpdateStatus::Downloading { version: version.clone(), percent });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    set_status(app, UpdateStatus::Ready { version });
    Ok(Some((update, bytes)))
}

fn install_when_idle(app: &AppHandle, update: Update, bytes: Vec<u8>) {
    let state = app.state::<AppState>().inner().clone();
    let version = update.version.clone();
    loop {
        while state.is_busy() {
            std::thread::sleep(IDLE_POLL);
        }
        set_status(app, UpdateStatus::Installing { version: version.clone() });
        std::thread::sleep(INSTALL_GRACE);
        if !state.is_busy() {
            break;
        }
        // Went on air during the grace period: wait for the next idle moment.
        set_status(app, UpdateStatus::Ready { version: version.clone() });
    }
    tracing::info!("installing update {version}");
    state.shutdown();
    // On Windows this starts the installer and exits; it relaunches the app.
    if let Err(e) = update.install(bytes) {
        tracing::error!("update install failed: {e}");
        set_status(app, UpdateStatus::Idle);
        return;
    }
    app.restart();
}

pub mod audio_cpal;
pub mod commands;
pub mod events;
pub mod logging;
pub mod power;
pub mod secrets;
pub mod storage;
pub mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager, WindowEvent};

use commands::AppState;
use events::{EVENT_RECORD_STATE, EVENT_STREAM_STATE};
use power::PowerInhibitor;
use storage::db::ProfileDb;
use storage::prefs::SettingsStorage;
use streamy_core::session::record_session::RecordSession;
use streamy_core::session::stream_session::StreamSession;

pub fn run() {
    let app_dir = get_app_data_dir();
    let _ = std::fs::create_dir_all(&app_dir);

    let log_dir = app_dir.join("logs");
    let _ = logging::init_logging(log_dir.clone());

    let db_path = app_dir.join("streamy.db");
    let db = ProfileDb::open(db_path).expect("Failed to initialize SQLite database");

    let prefs_path = app_dir.join("settings.json");
    let prefs = SettingsStorage::new(prefs_path);

    let power = Arc::new(PowerInhibitor::new());
    let app_handle_holder: Arc<parking_lot::Mutex<Option<tauri::AppHandle>>> =
        Arc::new(parking_lot::Mutex::new(None));

    // Stream state callback
    let holder_stream = app_handle_holder.clone();
    let stream_callback = Arc::new(move |state| {
        if let Some(ref handle) = *holder_stream.lock() {
            let _ = handle.emit(EVENT_STREAM_STATE, state);
        }
    });
    let stream_session = StreamSession::new(stream_callback);

    // Record state callback
    let holder_record = app_handle_holder.clone();
    let record_callback = Arc::new(move |state| {
        if let Some(ref handle) = *holder_record.lock() {
            let _ = handle.emit(EVENT_RECORD_STATE, state);
        }
    });
    let record_session = RecordSession::new(record_callback);

    let app_state = AppState {
        db: Arc::new(parking_lot::Mutex::new(db)),
        prefs: Arc::new(parking_lot::Mutex::new(prefs)),
        power,
        stream_session,
        record_session,
        app_handle: app_handle_holder.clone(),
        log_dir,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .manage(app_state)
        .setup(move |app| {
            *app_handle_holder.lock() = Some(app.handle().clone());
            let _ = tray::setup_tray(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Minimize to tray instead of closing: broadcast / record keeps running!
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_stream_state,
            commands::start_stream,
            commands::stop_stream,
            commands::update_now_playing,
            commands::set_mic_gain,
            commands::get_record_state,
            commands::start_recording,
            commands::stop_recording,
            commands::toggle_stream_recording,
            commands::list_input_devices,
            commands::get_settings,
            commands::save_settings,
            commands::list_profiles,
            commands::get_profile,
            commands::save_profile,
            commands::delete_profile,
            commands::clear_all_profiles,
            commands::get_lame_notice,
            commands::open_log_folder,
            commands::open_recordings_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn get_app_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("com.streamy.desktop");
        }
    }
    PathBuf::from(".streamy_desktop")
}

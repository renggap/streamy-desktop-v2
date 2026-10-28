pub mod audio_cpal;
pub mod audio_out;
pub mod commands;
pub mod events;
pub mod logging;
pub mod power;
pub mod secrets;
pub mod storage;
pub mod tray;
pub mod updater;

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager, WindowEvent};

use commands::AppState;
use events::{EVENT_RECORD_STATE, EVENT_STREAM_STATE};
use power::{Hold, PowerInhibitor};
use storage::db::ProfileDb;
use storage::prefs::SettingsStorage;
use streamy_core::audio::player::PlaylistPlayer;
use streamy_core::audio::switch::{InputControl, InputKind};
use streamy_core::core::teardown_state::{RecordState, StreamState};
use streamy_core::session::record_session::RecordSession;
use streamy_core::session::stream_session::StreamSession;

pub fn run() {
    // panic=abort + windows_subsystem=windows makes panics vanish silently.
    // Write every panic to crash.log next to the database before abort.
    // Direct fs write, not tracing: hook may run before init_logging, and the
    // non_blocking buffer is lost on abort.
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".into());
        let line = format!(
            "[{}] PANIC at {}: {}\n",
            chrono_free_timestamp(),
            location,
            payload
        );
        eprintln!("{line}");
        let mut path = get_app_data_dir();
        path.push("crash.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
    }));

    let app_dir = get_app_data_dir();
    let _ = std::fs::create_dir_all(&app_dir);

    let log_dir = app_dir.join("logs");
    let _ = logging::init_logging(log_dir.clone());

    let db_path = app_dir.join("streamy.db");
    let db = match ProfileDb::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            // A corrupt database must not make the app unlaunchable: keep the
            // file for recovery and start fresh.
            tracing::error!("profile database unreadable ({e}); starting with an empty one");
            let _ = std::fs::rename(&db_path, db_path.with_extension("db.corrupt"));
            ProfileDb::open(&db_path).expect("Failed to initialize SQLite database")
        }
    };

    let prefs_path = app_dir.join("settings.json");
    let prefs = SettingsStorage::new(prefs_path);

    let initial_settings = prefs.load();

    let power = Arc::new(PowerInhibitor::new());
    let app_handle_holder: Arc<parking_lot::Mutex<Option<tauri::AppHandle>>> =
        Arc::new(parking_lot::Mutex::new(None));

    // Stream state callback: keeps the sleep hold, tray tooltip and UI in step
    // with the session, whatever caused the change (UI, tray, failure).
    let holder_stream = app_handle_holder.clone();
    let power_stream = power.clone();
    let stream_callback = Arc::new(move |state: StreamState| {
        let busy = matches!(
            state,
            StreamState::Connecting | StreamState::Live { .. } | StreamState::Reconnecting { .. }
        );
        power_stream.set(Hold::Stream, busy);
        if let Some(ref handle) = *holder_stream.lock() {
            let tip = match &state {
                StreamState::Idle => "Streamy Desktop",
                StreamState::Connecting => "Streamy - connecting...",
                StreamState::Live { .. } => "Streamy - ON AIR",
                StreamState::Reconnecting { .. } => "Streamy - reconnecting...",
                StreamState::Error { .. } => "Streamy - stream error",
            };
            tray::set_tooltip(handle, tip);
            let _ = handle.emit(EVENT_STREAM_STATE, state);
        }
    });
    let stream_session = StreamSession::new(stream_callback);

    // Record state callback
    let holder_record = app_handle_holder.clone();
    let power_record = power.clone();
    let record_callback = Arc::new(move |state: RecordState| {
        power_record.set(Hold::Record, matches!(state, RecordState::Recording { .. }));
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
        mic_test: Arc::new(parking_lot::Mutex::new(None)),
        input: InputControl::new(
            if initial_settings.input_source == "playlist" { InputKind::Playlist } else { InputKind::Mic },
            initial_settings.mic_gain,
        ),
        player: PlaylistPlayer::new(),
        monitor: Arc::new(parking_lot::Mutex::new(None)),
        playlist_path: app_dir.join("playlist.json"),
    };
    // Saved gain applies from the first capture, not only after the slider moves.
    app_state.set_gain(initial_settings.mic_gain);
    app_state.input.set_noise_suppression(initial_settings.noise_suppression);
    app_state.restore_player(&initial_settings);

    // Player events arrive on its audio clock thread; handle them (file writes,
    // opening the speakers, IPC) on a worker so playback never stalls.
    let (player_tx, player_rx) = std::sync::mpsc::channel();
    app_state.player.set_event_handler(Arc::new(move |e| {
        let _ = player_tx.send(e);
    }));
    let worker_state = app_state.clone();
    let _ = std::thread::Builder::new()
        .name("streamy-player-events".into())
        .spawn(move || {
            for event in player_rx {
                worker_state.on_player_event(event);
            }
        });

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .manage(updater::UpdateTracker::default())
        .setup(move |app| {
            *app_handle_holder.lock() = Some(app.handle().clone());
            if let Err(e) = tray::setup_tray(app.handle()) {
                tracing::error!("tray setup failed: {e}");
            }
            if let Err(e) = commands::apply_autostart(app.handle(), initial_settings.launch_at_login) {
                tracing::warn!("autostart sync failed: {e}");
            }
            // The window is created hidden (tauri.conf) so "start minimized"
            // never flashes it on screen.
            if !initial_settings.start_minimized {
                tray::show_main_window(app.handle());
            }
            updater::spawn(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let busy = app.try_state::<AppState>().is_some_and(|st| {
                    st.stream_session.is_active()
                        || matches!(st.record_session.state(), RecordState::Recording { .. })
                });
                // Broadcast / recording keeps running in the tray; when idle, close quits.
                api.prevent_close();
                if busy {
                    let _ = window.hide();
                } else if let Some(st) = app.try_state::<AppState>() {
                    let st = st.inner().clone();
                    let handle = app.clone();
                    std::thread::spawn(move || {
                        st.shutdown();
                        handle.exit(0);
                    });
                } else {
                    app.exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::start_stream,
            commands::stop_stream,
            commands::update_now_playing,
            commands::set_mic_gain,
            commands::start_recording,
            commands::stop_recording,
            commands::start_mic_test,
            commands::stop_mic_test,
            commands::list_input_devices,
            commands::get_settings,
            commands::save_settings,
            commands::list_profiles,
            commands::get_profile,
            commands::save_profile,
            commands::delete_profile,
            commands::clear_all_profiles,
            commands::get_lame_notice,
            commands::get_app_info,
            commands::get_update_status,
            commands::open_log_folder,
            commands::open_recordings_folder,
            commands::set_input_source,
            commands::get_player,
            commands::playlist_pick_files,
            commands::playlist_add,
            commands::playlist_remove,
            commands::playlist_move,
            commands::playlist_clear,
            commands::player_play,
            commands::player_pause,
            commands::player_stop,
            commands::player_next,
            commands::player_previous,
            commands::player_seek,
            commands::player_set_volume,
            commands::player_set_options,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Last chance to close sockets and finalize recordings.
                if let Some(st) = app.try_state::<AppState>() {
                    st.shutdown();
                }
            }
        });
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

/// Local time "YYYY-MM-DD HH:MM:SS" without a chrono dependency.
fn chrono_free_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let rem = secs.rem_euclid(86_400);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m, d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's civil_from_days (public domain), also used by RecordSession.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

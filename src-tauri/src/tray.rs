//! System tray icon and minimize-to-tray handling.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, "show", "Show Streamy", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop Stream", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show_item, &stop_item, &quit_item])?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Streamy Desktop")
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "stop" => {
                if let Some(state) = app.try_state::<crate::commands::AppState>() {
                    state.stream_session.stop_and_shutdown(false);
                    state.power.release();
                }
            }
            "quit" => {
                if let Some(state) = app.try_state::<crate::commands::AppState>() {
                    state.stream_session.stop_and_shutdown(false);
                    state.record_session.stop_and_shutdown(false);
                    state.power.release();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { .. } = event {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}

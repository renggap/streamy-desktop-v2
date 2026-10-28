//! System tray icon and minimize-to-tray handling.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::commands::AppState;

pub const TRAY_ID: &str = "main";

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Run a session action off the event-loop thread: teardown joins audio and
/// network threads and must never stall the UI.
fn with_state(app: &AppHandle, f: impl FnOnce(AppState) + Send + 'static) {
    if let Some(state) = app.try_state::<AppState>() {
        let st = state.inner().clone();
        std::thread::spawn(move || f(st));
    }
}

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, "show", "Show Streamy", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop broadcast", true, None::<&str>)?;
    let stop_rec_item = MenuItem::with_id(app, "stop_rec", "Stop recording", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show_item, &stop_item, &stop_rec_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Streamy Desktop")
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "stop" => with_state(app, |st| st.stop_stream()),
            "stop_rec" => with_state(app, |st| st.stop_recording()),
            "quit" => {
                let handle = app.clone();
                with_state(app, move |st| {
                    st.shutdown();
                    handle.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    Ok(())
}

/// Reflect the broadcast state in the tray tooltip.
pub fn set_tooltip(app: &AppHandle, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

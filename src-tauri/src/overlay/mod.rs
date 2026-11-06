//! Overlay windows (`hud`, `tabpanel`) that sit over the game (SPEC §4).
//!
//! Both are separate topmost, transparent windows that never take focus from the game.
//! No graphics hooks, no input hooks.

mod win32;

use tauri::{AppHandle, Manager};

pub const HUD: &str = "hud";
pub const TAB_PANEL: &str = "tabpanel";

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    for label in [HUD, TAB_PANEL] {
        let Some(window) = app.get_webview_window(label) else {
            tracing::warn!("overlay window `{label}` is missing from tauri.conf.json");
            continue;
        };
        if label == HUD {
            // The HUD is display-only: clicks always pass through to the game.
            window.set_ignore_cursor_events(true)?;
        }
        // Must come after Tauri style changes like set_ignore_cursor_events, which rewrite the
        // extended style and would drop these bits. Re-apply after any such call (e.g. in M3).
        win32::make_non_activating(&window)?;
    }
    Ok(())
}

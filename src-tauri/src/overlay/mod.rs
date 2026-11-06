//! Overlay windows (`hud`, `tabpanel`) over the game (SPEC §4, §5.11).
//!
//! Both are separate topmost, transparent windows that never take focus from the game. No
//! graphics hooks, no input hooks. They're placed on the monitor League is on. The tab panel is
//! shown only while the scoreboard key is held with League in front, and only when the summoner
//! timer feature is turned on.

mod tab_watch;
pub mod win32;

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};
use ts_rs::TS;

use crate::state::AppState;
use tab_watch::TabWatch;
use win32::Rect;

pub const HUD: &str = "hud";
pub const TAB_PANEL: &str = "tabpanel";
pub const MODE_EVENT: &str = "overlay-mode";

/// Default tab panel spot next to the scoreboard's enemy rows, as a share of the monitor
/// (from the design at 1920×1080: x 1548, y 248).
const DEFAULT_OFFSET: (f64, f64) = (0.806, 0.23);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct OverlayMode {
    /// Calibration: the tab panel is shown and draggable, and the HUD shows a test banner.
    pub calibrating: bool,
}

#[derive(Default)]
pub struct OverlayRuntime {
    tab_watch: Option<TabWatch>,
    mode: OverlayMode,
}

pub type SharedOverlay = Mutex<OverlayRuntime>;

fn window(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    for label in [HUD, TAB_PANEL] {
        let Some(w) = window(app, label) else {
            tracing::warn!("overlay window `{label}` is missing from tauri.conf.json");
            continue;
        };
        if label == HUD {
            // The HUD is display-only: clicks always pass through to the game.
            w.set_ignore_cursor_events(true)?;
        }
        // Must come after Tauri style changes like set_ignore_cursor_events, which rewrite the
        // extended style and would drop these bits.
        win32::make_non_activating(&w)?;
    }
    Ok(())
}

/// The monitor to put overlays on: League's, else the primary monitor.
fn target_monitor(app: &AppHandle) -> Option<Rect> {
    win32::league_monitor().or_else(|| {
        let m = app.primary_monitor().ok().flatten()?;
        Some(Rect { x: m.position().x, y: m.position().y, width: m.size().width as i32, height: m.size().height as i32 })
    })
}

fn resolution_key(r: &Rect) -> String {
    format!("{}x{}", r.width, r.height)
}

/// Tab panel position: the calibrated offset for this resolution, else the default spot.
pub fn tabpanel_position(monitor: &Rect, offsets: &std::collections::BTreeMap<String, (i32, i32)>) -> (i32, i32) {
    let (dx, dy) = offsets.get(&resolution_key(monitor)).copied().unwrap_or((
        (monitor.width as f64 * DEFAULT_OFFSET.0) as i32,
        (monitor.height as f64 * DEFAULT_OFFSET.1) as i32,
    ));
    (monitor.x + dx, monitor.y + dy)
}

fn place(app: &AppHandle) {
    let Some(monitor) = target_monitor(app) else { return };
    if let Some(hud) = window(app, HUD) {
        let _ = hud.set_position(PhysicalPosition::new(monitor.x, monitor.y));
        let _ = hud.set_size(PhysicalSize::new(monitor.width as u32, monitor.height as u32));
    }
    if let Some(panel) = window(app, TAB_PANEL) {
        let offsets = app.state::<AppState>().config.lock().unwrap().tabpanel_offsets.clone();
        let (x, y) = tabpanel_position(&monitor, &offsets);
        let _ = panel.set_position(PhysicalPosition::new(x, y));
    }
}

/// Shows or hides the tab panel. Shown means clickable; hidden overlays never take input.
pub fn set_tabpanel_visible(app: &AppHandle, visible: bool) {
    let Some(panel) = window(app, TAB_PANEL) else { return };
    // set_ignore_cursor_events rewrites the extended style, so the no-focus bits are re-applied
    // after it every time.
    let result = if visible {
        panel
            .set_ignore_cursor_events(false)
            .and_then(|_| win32::make_non_activating(&panel))
            .and_then(|_| win32::show_no_activate(&panel))
    } else {
        panel
            .hide()
            .and_then(|_| panel.set_ignore_cursor_events(true))
            .and_then(|_| win32::make_non_activating(&panel))
    };
    if let Err(e) = result {
        tracing::warn!("tab panel visibility change failed: {e}");
    }
}

/// The scoreboard key went down or up with League in front. Ignored while calibrating, so the
/// calibration panel stays put.
pub fn scoreboard_key(app: &AppHandle, held: bool) {
    if app.state::<AppState>().overlay.lock().unwrap().mode.calibrating {
        return;
    }
    set_tabpanel_visible(app, held);
}

fn set_hud_visible(app: &AppHandle, visible: bool) {
    let Some(hud) = window(app, HUD) else { return };
    let result = if visible { win32::show_no_activate(&hud) } else { hud.hide() };
    if let Err(e) = result {
        tracing::warn!("HUD visibility change failed: {e}");
    }
}

fn emit_mode(app: &AppHandle, mode: OverlayMode) {
    if let Err(e) = app.emit(MODE_EVENT, mode) {
        tracing::warn!("failed to emit {MODE_EVENT}: {e}");
    }
}

/// A game started: place overlays on its monitor and, with the timer feature on, watch the
/// scoreboard key.
pub fn game_started(app: &AppHandle) {
    place(app);
    let state = app.state::<AppState>();
    let (timers_on, key) = {
        let c = state.config.lock().unwrap();
        (c.flags.overlay_timers, c.scoreboard_key.clone())
    };
    let mut rt = state.overlay.lock().unwrap();
    if timers_on && rt.tab_watch.is_none() {
        match win32::virtual_key(&key) {
            Some(vk) => rt.tab_watch = Some(TabWatch::start(app.clone(), vk)),
            None => tracing::warn!("scoreboard key {key:?} isn't supported; tab panel disabled"),
        }
    }
}

/// The game ended: stop watching keys and hide the overlays (unless calibrating).
pub fn game_ended(app: &AppHandle) {
    let state = app.state::<AppState>();
    let calibrating = {
        let mut rt = state.overlay.lock().unwrap();
        rt.tab_watch = None; // dropping stops the thread
        rt.mode.calibrating
    };
    if !calibrating {
        set_tabpanel_visible(app, false);
        set_hud_visible(app, false);
    }
}

/// Calibration: show the tab panel (draggable) and a HUD test banner. Works with or without a
/// game running; with one, it's also the full-screen check (can the overlay be seen?).
pub fn start_calibration(app: &AppHandle) {
    app.state::<AppState>().overlay.lock().unwrap().mode.calibrating = true;
    place(app);
    set_hud_visible(app, true);
    set_tabpanel_visible(app, true);
    emit_mode(app, OverlayMode { calibrating: true });
}

/// Ends calibration. With `save`, stores the tab panel's position for this resolution.
pub fn finish_calibration(app: &AppHandle, save: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    if save {
        let monitor = target_monitor(app).ok_or("no monitor found")?;
        let panel = window(app, TAB_PANEL).ok_or("tab panel window is missing")?;
        let pos = panel.outer_position().map_err(|e| e.to_string())?;
        let mut config = state.config.lock().unwrap();
        config.tabpanel_offsets.insert(resolution_key(&monitor), (pos.x - monitor.x, pos.y - monitor.y));
        config.save(&state.config_path).map_err(|e| format!("couldn't save settings: {e}"))?;
    }
    state.overlay.lock().unwrap().mode.calibrating = false;
    set_tabpanel_visible(app, false);
    set_hud_visible(app, false);
    emit_mode(app, OverlayMode { calibrating: false });
    Ok(())
}

pub fn mode(app: &AppHandle) -> OverlayMode {
    app.state::<AppState>().overlay.lock().unwrap().mode
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn tab_panel_uses_calibration_for_the_resolution_else_the_default() {
        let monitor = Rect { x: 1920, y: 0, width: 1920, height: 1080 };
        let mut offsets = BTreeMap::new();
        assert_eq!(tabpanel_position(&monitor, &offsets), (1920 + 1547, 248));
        offsets.insert("1920x1080".to_string(), (1500, 300));
        assert_eq!(tabpanel_position(&monitor, &offsets), (3420, 300));
        let qhd = Rect { x: 0, y: 0, width: 2560, height: 1440 };
        assert_eq!(tabpanel_position(&qhd, &offsets), (2063, 331), "other resolutions keep the default");
    }
}

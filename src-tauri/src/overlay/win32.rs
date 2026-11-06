//! Win32 helpers for the overlay windows (SPEC §4, §5.11).
//!
//! Compliance: League's window is found by its class name only (`FindWindowW`,
//! `GetForegroundWindow` + `GetClassNameW`). No process handle is ever opened. Keys are read
//! passively with `GetAsyncKeyState`; nothing is written to the input stream.

/// League's game window class.
pub const LEAGUE_CLASS: &str = "RiotWindowClass";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[cfg(windows)]
mod imp {
    use super::{LEAGUE_CLASS, Rect};
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, SW_SHOWNOACTIVATE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    use windows::core::{HSTRING, PCWSTR};

    fn hwnd_of(window: &WebviewWindow) -> tauri::Result<HWND> {
        // Tauri may use a different `windows` crate version, so rebuild the handle from its pointer.
        Ok(HWND(window.hwnd()?.0))
    }

    pub fn make_non_activating(window: &WebviewWindow) -> tauri::Result<()> {
        let hwnd = hwnd_of(window)?;
        let extra = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        // SAFETY: `hwnd` is a live top-level window owned by this process.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | extra);
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
            if let Err(e) = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags) {
                tracing::warn!("SetWindowPos on overlay `{}` failed: {e}", window.label());
            }
        }
        Ok(())
    }

    /// Shows the window without activating it, so the game keeps focus.
    pub fn show_no_activate(window: &WebviewWindow) -> tauri::Result<()> {
        let hwnd = hwnd_of(window)?;
        // SAFETY: as above. The return value is the previous visibility, not an error.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }

    fn league_window() -> Option<HWND> {
        let class = HSTRING::from(LEAGUE_CLASS);
        // SAFETY: plain window lookup by class name.
        unsafe { FindWindowW(&class, PCWSTR::null()).ok().filter(|h| !h.is_invalid()) }
    }

    pub fn league_in_front() -> bool {
        // SAFETY: reads the foreground window's class name into a local buffer.
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_invalid() {
                return false;
            }
            let mut buf = [0u16; 64];
            let n = GetClassNameW(fg, &mut buf);
            n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == LEAGUE_CLASS
        }
    }

    pub fn key_down(vk: i32) -> bool {
        // SAFETY: passive read of the key's state. High bit set = held right now.
        unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
    }

    /// The monitor League's window is on, if the game is running.
    pub fn league_monitor() -> Option<Rect> {
        let hwnd = league_window()?;
        // SAFETY: `info` is a properly sized local struct.
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return None;
            }
            let r = info.rcMonitor;
            Some(Rect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top })
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Rect;
    use tauri::WebviewWindow;

    pub fn make_non_activating(_: &WebviewWindow) -> tauri::Result<()> {
        Ok(())
    }
    pub fn show_no_activate(window: &WebviewWindow) -> tauri::Result<()> {
        window.show()
    }
    pub fn league_in_front() -> bool {
        false
    }
    pub fn key_down(_: i32) -> bool {
        false
    }
    pub fn league_monitor() -> Option<Rect> {
        None
    }
}

pub use imp::{key_down, league_in_front, league_monitor, make_non_activating, show_no_activate};

/// Virtual-key code for a scoreboard key name from Settings ("Tab", "O", "F1", "Space", "`").
pub fn virtual_key(name: &str) -> Option<i32> {
    let n = name.trim();
    match n.to_ascii_lowercase().as_str() {
        "tab" => return Some(0x09),
        "space" => return Some(0x20),
        "`" | "backquote" | "grave" => return Some(0xC0),
        _ => {}
    }
    let mut chars = n.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(c.to_ascii_uppercase() as i32);
    }
    let f: u8 = n.strip_prefix(['F', 'f'])?.parse().ok()?;
    (1..=12).contains(&f).then(|| 0x70 + i32::from(f) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_to_virtual_keys() {
        assert_eq!(virtual_key("Tab"), Some(0x09));
        assert_eq!(virtual_key("tab"), Some(0x09));
        assert_eq!(virtual_key("o"), Some('O' as i32));
        assert_eq!(virtual_key("7"), Some('7' as i32));
        assert_eq!(virtual_key("F1"), Some(0x70));
        assert_eq!(virtual_key("f12"), Some(0x7B));
        assert_eq!(virtual_key("Space"), Some(0x20));
        assert_eq!(virtual_key("`"), Some(0xC0));
        assert_eq!(virtual_key("F13"), None);
        assert_eq!(virtual_key("Ctrl"), None);
        assert_eq!(virtual_key(""), None);
    }
}

//! Win32 window styles for the overlay windows.

use tauri::WebviewWindow;

/// Adds `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` so clicking the window never takes focus from the
/// game and the window stays out of Alt+Tab.
#[cfg(windows)]
pub fn make_non_activating(window: &WebviewWindow) -> tauri::Result<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    // Tauri may use a different `windows` crate version, so rebuild the handle from its raw pointer.
    let hwnd = HWND(window.hwnd()?.0);
    let extra = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;

    // SAFETY: `hwnd` is a live top-level window owned by this process.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | extra);
        // Make Windows apply the changed extended style without moving or activating the window.
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
        if let Err(e) = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags) {
            tracing::warn!("SetWindowPos on overlay `{}` failed: {e}", window.label());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn make_non_activating(_window: &WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

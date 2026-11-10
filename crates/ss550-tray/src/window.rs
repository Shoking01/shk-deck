//! Hiding and restoring the native window.
//!
//! GPUI has no "hide" call. Its only window controls are minimize, which
//! leaves a taskbar button behind, and remove, which destroys the window and
//! with it the controller that owns the deck. To behave like common Stream
//! Deck software the window has to leave the screen and the taskbar while
//! the app keeps running, so the native handle is used directly.

use crate::TrayError;

/// Hides the native window without destroying it.
///
/// The window keeps its size, its place and its state, so it comes back
/// exactly where it was.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// # let hwnd = 0isize;
/// ss550_tray::hide_window(hwnd)?;
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn hide_window(hwnd: isize) -> Result<(), TrayError> {
    set_visible(hwnd, false)
}

/// Shows the native window again.
///
/// A window that was minimized comes back restored, which is what the user
/// expects when they ask for the controller from the tray.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// # let hwnd = 0isize;
/// ss550_tray::show_window(hwnd)?;
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn show_window(hwnd: isize) -> Result<(), TrayError> {
    set_visible(hwnd, true)
}

/// Whether the native window is on the screen right now.
///
/// # Example
///
/// ```no_run
/// # let hwnd = 0isize;
/// if ss550_tray::window_is_visible(hwnd) {
///     println!("the controller is on screen");
/// }
/// ```
#[cfg(windows)]
pub fn window_is_visible(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    let handle = HWND(hwnd as *mut std::ffi::c_void);
    // SAFETY: the handle is only read, and an invalid handle is reported as
    // "not visible" instead of being used for anything else.
    unsafe { IsWindowVisible(handle).as_bool() }
}

/// Hides or shows the native window.
#[cfg(windows)]
fn set_visible(hwnd: isize, visible: bool) -> Result<(), TrayError> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, ShowWindow, SW_HIDE, SW_RESTORE};

    let handle = HWND(hwnd as *mut std::ffi::c_void);
    // SAFETY: the handle comes from the window GPUI created and is only used
    // while that window is alive, which `IsWindow` checks first.
    unsafe {
        if !IsWindow(Some(handle)).as_bool() {
            return Err(TrayError::Windows(
                "the controller window no longer exists".to_string(),
            ));
        }
        // `ShowWindow` answers with the visibility the window had before,
        // which is of no use here.
        let _ = ShowWindow(handle, if visible { SW_RESTORE } else { SW_HIDE });
    }
    Ok(())
}

/// Hiding a window is Windows behaviour; elsewhere the app has no tray.
#[cfg(not(windows))]
pub fn hide_window(_hwnd: isize) -> Result<(), TrayError> {
    Err(TrayError::Unsupported)
}

/// Showing a window again is Windows behaviour; elsewhere the app has no tray.
#[cfg(not(windows))]
pub fn show_window(_hwnd: isize) -> Result<(), TrayError> {
    Err(TrayError::Unsupported)
}

/// Everywhere but on Windows the window is always on screen.
#[cfg(not(windows))]
pub fn window_is_visible(_hwnd: isize) -> bool {
    true
}

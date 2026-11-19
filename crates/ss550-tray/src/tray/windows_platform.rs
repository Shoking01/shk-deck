//! The Windows notification-area icon.
//!
//! `tray-icon` builds a hidden window on the thread that creates it and
//! dispatches the menu through that window's message loop, so the icon needs
//! a thread of its own with a message loop of its own. The controller's
//! window belongs to the UI thread, and the two never share messages.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_APP, WM_QUIT,
};

use crate::icon::{deck_icon_rgba, ICON_SIZE};
use crate::tray::{TrayCommand, TrayError, TrayMessage};

/// The entry that brings the controller window back.
const SHOW_LABEL: &str = "Show controller";

/// The entry that leaves the app for good.
const QUIT_LABEL: &str = "Quit";

/// A message that only wakes the thread up, so it reads its channel.
const WAKE: u32 = WM_APP + 1;

/// Creates the icon and runs its message loop until the app stops it.
pub(crate) fn run(
    tooltip: String,
    commands: Sender<TrayCommand>,
    messages: Receiver<TrayMessage>,
    ready: Sender<Result<u32, TrayError>>,
    cancelled: Arc<AtomicBool>,
) {
    if cancelled.load(Ordering::SeqCst) {
        return;
    }

    let menu = Menu::new();
    let show = MenuItem::new(SHOW_LABEL, true, None);
    let quit = MenuItem::new(QUIT_LABEL, true, None);
    let show_id = show.id().clone();
    let quit_id = quit.id().clone();

    if let Err(error) = menu.append(&show).and_then(|()| menu.append(&quit)) {
        let _ = ready.send(Err(TrayError::Tray(error.to_string())));
        return;
    }

    let icon = match Icon::from_rgba(deck_icon_rgba(), ICON_SIZE, ICON_SIZE) {
        Ok(icon) => icon,
        Err(error) => {
            let _ = ready.send(Err(TrayError::Tray(format!(
                "the icon could not be drawn: {error}"
            ))));
            return;
        }
    };

    let tray = match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip)
        .with_icon(icon)
        .build()
    {
        Ok(tray) => tray,
        Err(error) => {
            let _ = ready.send(Err(TrayError::Tray(error.to_string())));
            return;
        }
    };

    // SAFETY: the call only reads the id of the calling thread.
    let thread_id = unsafe { GetCurrentThreadId() };
    if ready.send(Ok(thread_id)).is_err() {
        // Nobody is waiting for the icon any more: leave none behind.
        return;
    }
    if cancelled.load(Ordering::SeqCst) {
        return;
    }

    loop {
        let mut message = MSG::default();
        // SAFETY: `message` is a valid MSG for the duration of the call, and
        // a null window handle asks for every message of this thread.
        let received = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if received <= 0 {
            // WM_QUIT, or an error: the app is leaving.
            break;
        }
        // SAFETY: the message came out of this thread's own queue.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        while let Ok(request) = messages.try_recv() {
            match request {
                TrayMessage::SetTooltip(text) => {
                    let _ = tray.set_tooltip(Some(text));
                }
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == show_id {
                let _ = commands.send(TrayCommand::Show);
            } else if event.id == quit_id {
                let _ = commands.send(TrayCommand::Quit);
            }
        }
    }
}

/// Stops the icon's message loop.
pub(crate) fn stop(thread_id: u32) {
    // SAFETY: the thread id belongs to the tray thread, which is still alive
    // while the host is being dropped.
    unsafe {
        let _ = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
    }
}

/// Wakes the tray thread, so it reads the messages it was sent.
pub(crate) fn wake(thread_id: u32) {
    // SAFETY: the thread id belongs to the tray thread, which is still alive
    // while the host is alive.
    unsafe {
        let _ = PostThreadMessageW(thread_id, WAKE, WPARAM(0), LPARAM(0));
    }
}

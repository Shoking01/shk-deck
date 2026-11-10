//! The notification area on platforms that do not have one.
//!
//! The controller is Windows-first: the icon, the hidden window and the
//! logon entry are Windows behaviour. Everywhere else the app still runs,
//! but there is no notification-area icon to live in.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use crate::tray::{TrayCommand, TrayError, TrayMessage};

/// Reports that there is no notification area here.
pub(crate) fn run(
    _tooltip: String,
    _commands: Sender<TrayCommand>,
    _messages: Receiver<TrayMessage>,
    ready: Sender<Result<u32, TrayError>>,
    _cancelled: Arc<AtomicBool>,
) {
    let _ = ready.send(Err(TrayError::Unsupported));
}

/// There is no message loop to stop.
pub(crate) fn stop(_thread_id: u32) {}

/// There is no message loop to wake.
pub(crate) fn wake(_thread_id: u32) {}

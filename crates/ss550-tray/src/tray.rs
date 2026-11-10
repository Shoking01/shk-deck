//! The notification-area icon and its menu.
//!
//! The icon lives on a thread of its own. Windows asks for it: the icon is
//! a hidden window that belongs to the thread that created it, and only that
//! thread can dispatch the messages that carry the menu clicks. The
//! controller's own window belongs to the UI thread, and the two never share
//! a message queue.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::TrayError;

#[cfg(not(windows))]
mod other_platform;
#[cfg(windows)]
mod windows_platform;

#[cfg(not(windows))]
use other_platform as platform;
#[cfg(windows)]
use windows_platform as platform;

/// How long the icon gets to come up before the app gives up on it.
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// What the tray menu can ask the controller to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// Bring the controller window back.
    Show,
    /// Leave the controller for good.
    Quit,
}

/// A request for the thread that owns the icon.
#[derive(Debug)]
enum TrayMessage {
    /// Replace the tooltip of the icon.
    ///
    /// Off Windows there is no icon to rename, so nothing reads the text
    /// there; the message itself still travels, so the API behaves the same
    /// on every platform.
    #[cfg_attr(not(windows), allow(dead_code))]
    SetTooltip(String),
}

/// The notification-area icon, running on its own thread.
pub struct TrayHost {
    /// What the user asked for in the menu.
    commands: Receiver<TrayCommand>,
    /// Requests for the thread that owns the icon.
    messages: Sender<TrayMessage>,
    /// The thread that owns the icon, so it can be woken and stopped.
    thread_id: u32,
    /// The thread itself, joined when the host is dropped.
    thread: Option<JoinHandle<()>>,
}

impl TrayHost {
    /// Creates the icon and starts listening for the menu.
    ///
    /// The menu has two entries: **Show controller** and **Quit**.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> Result<(), ss550_tray::TrayError> {
    /// let tray = ss550_tray::TrayHost::spawn("StreamCraft SS550 Controller")?;
    /// assert!(tray.try_recv().is_none());
    /// # Ok(())
    /// # }
    /// ```
    pub fn spawn(tooltip: &str) -> Result<Self, TrayError> {
        let tooltip = tooltip.to_string();
        let (command_sender, commands) = mpsc::channel();
        let (messages, message_receiver) = mpsc::channel();
        let (ready_sender, ready) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));

        let thread = std::thread::Builder::new()
            .name("ss550-tray".to_string())
            .spawn({
                let cancelled = Arc::clone(&cancelled);
                move || {
                    platform::run(
                        tooltip,
                        command_sender,
                        message_receiver,
                        ready_sender,
                        cancelled,
                    );
                }
            })
            .map_err(|error| {
                TrayError::Tray(format!("the tray thread could not start: {error}"))
            })?;

        match ready.recv_timeout(START_TIMEOUT) {
            Ok(Ok(thread_id)) => Ok(Self {
                commands,
                messages,
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                // The thread is on its way out; wait for it so nothing is
                // left running behind a failed icon.
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                // The thread is stuck; tell it to leave as soon as it can.
                cancelled.store(true, Ordering::SeqCst);
                Err(TrayError::Timeout)
            }
        }
    }

    /// Takes the next thing the user asked for in the tray, if any.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> Result<(), ss550_tray::TrayError> {
    /// let tray = ss550_tray::TrayHost::spawn("StreamCraft SS550 Controller")?;
    /// if let Some(ss550_tray::TrayCommand::Quit) = tray.try_recv() {
    ///     println!("the user asked to leave");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn try_recv(&self) -> Option<TrayCommand> {
        match self.commands.try_recv() {
            Ok(command) => Some(command),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Replaces the tooltip of the icon.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> Result<(), ss550_tray::TrayError> {
    /// let tray = ss550_tray::TrayHost::spawn("StreamCraft SS550 Controller")?;
    /// tray.set_tooltip("StreamCraft SS550 Controller - deck connected")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_tooltip(&self, tooltip: &str) -> Result<(), TrayError> {
        self.messages
            .send(TrayMessage::SetTooltip(tooltip.to_string()))
            .map_err(|_| TrayError::Tray("the tray thread is gone".to_string()))?;
        platform::wake(self.thread_id);
        Ok(())
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        // Ask the thread to leave its message loop and wait for it, so the
        // icon leaves the notification area with the app.
        platform::stop(self.thread_id);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

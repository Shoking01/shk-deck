//! System tray host and window hiding for the SS550 controller.
//!
//! Common Stream Deck software lives in the notification area: no window in
//! the way, the deck stays awake, and the bound keys keep working while the
//! window is hidden. This crate provides that behaviour.
//!
//! It is also the **one** place in the project that is allowed to call
//! `unsafe` code: it owns the Win32 calls that hide and restore the native
//! window, and the icon that lives in the notification area. Every other
//! crate keeps `#![deny(unsafe_code)]`.
//!
//! # Example
//!
//! ```no_run
//! use ss550_tray::TrayHost;
//!
//! let tray = TrayHost::spawn("StreamCraft SS550 Controller")?;
//!
//! // In the application's own loop:
//! while let Some(command) = tray.try_recv() {
//!     match command {
//!         ss550_tray::TrayCommand::Show => { /* bring the window back */ }
//!         ss550_tray::TrayCommand::Quit => { /* leave for good */ }
//!     }
//! }
//! # Ok::<(), ss550_tray::TrayError>(())
//! ```

// The Win32 calls that hide a window and run the icon's message loop live in
// this crate, and nowhere else in the project.
#![allow(unsafe_code)]

pub mod autostart;
pub mod icon;
mod tray;
mod window;

pub use tray::{TrayCommand, TrayHost};
pub use window::{hide_window, show_window, window_is_visible};

/// Everything that can go wrong while setting the tray up.
#[derive(Debug)]
pub enum TrayError {
    /// The notification-area icon could not be created.
    Tray(String),
    /// A Win32 call on the window failed.
    Windows(String),
    /// The registry could not be read or written.
    Registry(String),
    /// The tray thread did not come up in time.
    Timeout,
    /// This platform has no notification area.
    Unsupported,
}

impl std::fmt::Display for TrayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrayError::Tray(message) => write!(formatter, "the tray icon failed: {message}"),
            TrayError::Windows(message) => write!(formatter, "the window call failed: {message}"),
            TrayError::Registry(message) => {
                write!(formatter, "the registry call failed: {message}")
            }
            TrayError::Timeout => write!(formatter, "the tray icon did not start in time"),
            TrayError::Unsupported => write!(formatter, "this platform has no notification area"),
        }
    }
}

impl std::error::Error for TrayError {}

//! # ss550-driver
//!
//! A Rust driver for the Redragon StreamCraft SS550 v3 HID controller (VID 0x0200 / PID 0x3000).
//!
//! ## Overview
//!
//! This crate provides a high-level API to control the 15 LCD buttons on the StreamCraft SS550 v3:
//! - Display images (JPEG, PNG, etc.) on any button
//! - Set brightness (0-100%)
//! - Clear buttons individually or all at once
//! - Query device identity
//! - Read key presses and releases ([`Device::poll_events`])
//!
//! ## Two faces, one gate
//!
//! [`Ss550Deck`] is the model behind the [`deck_api::Deck`] seam: key count,
//! grid, image spec, capabilities and typed refusals, with no mention of bytes.
//! [`Device`] is the concrete handle the first milestone exposed and the window
//! still uses. Both sit on the same connection, and both write through the same
//! verified-command gate — the seam is an adapter, not a second driver.
//!
//! Every byte goes through a [`deck_api::Transport`], so the whole stack runs
//! over a fake in tests and in the simulator, and a plain `cargo test` never
//! opens a device.
//!
//! ## Example
//!
//! ```no_run
//! use ss550_driver::{Device, ButtonImage, Key};
//!
//! fn main() -> Result<(), ss550_driver::Ss550Error> {
//!     // Connect to the device
//!     let mut device = Device::connect()?;
//!
//!     // Initialize: wake + set brightness 0 (driver does this automatically)
//!     device.initialize()?;
//!
//!     // Set a white image on the top-left button
//!     let image = ButtonImage::white(Key::TopLeft as u8)?;
//!     device.queue_image(image)?;
//!     device.flush()?;
//!
//!     Ok(())
//! }
//! ```
//!

// `hidapi` keeps the FFI inside its own crate; nothing here needs to call it.
#![deny(unsafe_code)]

mod connection;
mod deck;
mod error;
mod hid_transport;
mod image;
mod keys;

pub use connection::{
    ensure_verified, ButtonEvent, Command, Device, LONG_CONNECT_TIMEOUT, PID, VID,
};
pub use deck::Ss550Deck;
pub use error::Ss550Error;
pub use image::{ButtonImage, Rotation};
pub use keys::Key;
pub use ss550_protocol::ButtonState;

/// Convenience re-export of commonly used types.
pub mod prelude {
    pub use crate::{Device, Key, Ss550Error};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_variants() {
        assert_eq!(Key::BottomLeft as u8, 0);
        assert_eq!(Key::TopRight as u8, 14);
    }

    #[test]
    fn test_white_image_creation() {
        let img = ButtonImage::white(0).unwrap();
        assert!(!img.jpeg.is_empty());
        assert_eq!(img.key, 0);
    }

    #[test]
    fn test_red_image_creation() {
        let img = ButtonImage::red(5).unwrap();
        assert!(!img.jpeg.is_empty());
        assert_eq!(img.key, 5);
    }

    #[test]
    fn test_open_deck_mapping() {
        // Top-left (OpenDeck key 10) should map to firmware key 11
        assert_eq!(keys::open_deck_to_firmware(10), 11);
        // Bottom-right (OpenDeck key 4) should map to firmware key 5
        assert_eq!(keys::open_deck_to_firmware(4), 5);
    }

    #[test]
    fn the_handles_can_still_cross_threads() {
        // The window keeps a `Device` in an entity it owns. Putting the bytes
        // behind a boxed transport must not cost the handle its `Send`, or the
        // seam would be a regression for the one caller that already exists.
        fn assert_send<T: Send>() {}
        assert_send::<Device>();
        assert_send::<Ss550Deck>();
    }
}

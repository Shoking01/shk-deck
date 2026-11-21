//! Finding the deck without walking the bus every time.
//!
//! `HidApi::new()` enumerates **every** HID device on the machine, opening each
//! one to read its attributes. That is an expensive call, and a hazardous one: a
//! device that does not answer during the walk can queue every other HID client
//! behind it, the pointer included — which is why the window must never do it on
//! a timer.
//!
//! [`Locator`] pays for that walk **once per process** and then remembers the
//! device path it opened. Every later attempt — a disconnect, a deck that
//! reappears, a reboot of the app's own connection — asks for that one path,
//! which is a single `CreateFile` and no enumeration at all. Only when the path
//! stops working does it walk again, because that is the one case where the deck
//! may have moved.
//!
//! The locator lives on the thread that owns the HID context: `HidApi` is not
//! `Send`, and the deck handle it opens is not either.

use std::time::Duration;

use hidapi::HidApi;

use crate::connection::Device;
use crate::deck::Ss550Deck;
use crate::error::Ss550Error;

/// The deck's remembered address, and the one HID context this process builds.
pub struct Locator {
    /// The context every open goes through. Built once, here, with the one walk
    /// the process performs.
    api: HidApi,
    /// Where the deck was last found. `None` until it has been opened once.
    path: Option<String>,
}

impl Locator {
    /// Builds the locator, seeded with a path remembered from a previous run.
    ///
    /// The seed is what a start at logon uses: the walk still happens here —
    /// `HidApi::new` always enumerates — but every attempt after it is cheap.
    pub fn new(remembered: Option<String>) -> Result<Self, Ss550Error> {
        let api = HidApi::new().map_err(|error| Ss550Error::HidOpenError(error.to_string()))?;
        Ok(Self {
            api,
            path: remembered.filter(|path| !path.trim().is_empty()),
        })
    }

    /// Where the deck was last found, when it has been found once.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Opens the deck, by the remembered path when there is one.
    ///
    /// A path that no longer opens is forgotten and discovered again: the walk
    /// is paid once per moved deck, not once per retry.
    pub fn connect(&mut self, timeout: Duration) -> Result<Ss550Deck, Ss550Error> {
        if let Some(path) = self.path.clone() {
            if let Ok(device) = Device::connect_at_path_with(&mut self.api, &path, timeout) {
                let deck = Ss550Deck::from_device(device);
                self.path = deck.device_path();
                return Ok(deck);
            }
            self.path = None;
        }
        let deck = Ss550Deck::connect_with_api_timeout(&mut self.api, timeout)?;
        self.path = deck.device_path();
        Ok(deck)
    }
}

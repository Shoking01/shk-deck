//! The deck trait: what a deck can do.

use std::time::Duration;

use crate::error::DeckError;
use crate::types::{Capabilities, Geometry, ImageSpec, KeyEvent, KeyId};

/// A deck: the semantics of one streamdeck model.
///
/// Everything a caller needs to adapt to the deck in front of it is here, as
/// data or as a typed error. Nothing in this trait is about bytes: that is
/// [`Transport`](crate::Transport)'s job, and a deck built on one keeps its
/// protocol to itself.
///
/// An implementation that cannot do something returns
/// [`DeckError::Unsupported`]. It does not accept the call and quietly do
/// nothing: a caller that cannot tell the difference between "done" and "not
/// possible" has no way to tell the user anything useful.
///
/// The same rule covers values: a brightness outside the accepted range is
/// refused with [`DeckError::ValueOutOfRange`], never clamped.
pub trait Deck {
    /// The shape of the deck's keys.
    fn geometry(&self) -> Geometry;

    /// The image the deck accepts for one key.
    fn image_spec(&self) -> ImageSpec;

    /// What the deck can do.
    fn capabilities(&self) -> Capabilities;

    /// The deck's identity string, as the device reports it.
    fn identity(&self) -> Result<String, DeckError>;

    /// Sets the display brightness, from
    /// [`VALUE_MIN`](crate::VALUE_MIN) to [`VALUE_MAX`](crate::VALUE_MAX).
    ///
    /// A value outside that range is refused with
    /// [`DeckError::ValueOutOfRange`]: the seam does not clamp it, because a
    /// clamped request is a request that changed without telling anyone.
    fn set_brightness(&mut self, value: u8) -> Result<(), DeckError>;

    /// Paints one key with a JPEG image.
    ///
    /// The image must match the deck's [`ImageSpec`]; a deck that takes
    /// another size or a bigger payload refuses it with a typed error.
    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError>;

    /// Clears one key back to blank.
    fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError>;

    /// Clears every key back to blank.
    fn clear_all(&mut self) -> Result<(), DeckError>;

    /// Puts the deck to sleep.
    ///
    /// A deck that cannot sleep refuses with
    /// [`DeckError::Unsupported`], naming
    /// [`DeckOperation::Sleep`](crate::DeckOperation::Sleep).
    fn sleep(&mut self) -> Result<(), DeckError>;

    /// Keeps the deck from resetting itself while the host is idle.
    ///
    /// A deck that advertises
    /// [`Capabilities::keep_alive`](crate::Capabilities::keep_alive) implements
    /// this; the caller calls it from its main loop and the deck is free to
    /// throttle the write on its own. A deck with no idle reset does not
    /// advertise the capability, and this default refuses with
    /// [`DeckError::Unsupported`], naming
    /// [`DeckOperation::KeepAlive`](crate::DeckOperation::KeepAlive) — a deck
    /// that claims the capability and forgets to override this is caught by the
    /// conformance suite instead of silently doing nothing.
    fn keep_alive(&mut self) -> Result<(), DeckError> {
        Err(DeckError::Unsupported {
            operation: crate::DeckOperation::KeepAlive,
        })
    }

    /// Moves one dial to `value`, from
    /// [`VALUE_MIN`](crate::VALUE_MIN) to [`VALUE_MAX`](crate::VALUE_MAX).
    ///
    /// A deck with no dials refuses with [`DeckError::Unsupported`], naming
    /// [`DeckOperation::SetDial`](crate::DeckOperation::SetDial); a dial that
    /// does not exist is refused with [`DeckError::DialOutOfRange`].
    fn set_dial(&mut self, dial: u8, value: u8) -> Result<(), DeckError>;

    /// Reads the key events waiting on the deck.
    ///
    /// A timeout with nothing waiting is an empty list, not an error.
    fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError>;

    /// Reopens the device after a disconnect.
    fn reconnect(&mut self) -> Result<(), DeckError>;
}

/// A deck held behind one box.
///
/// This is the handle for code that knows "a deck" but not which one: the
/// window, the engine and the action runner can all hold a `DeckHandle`
/// without naming a model. The handle is a [`Deck`] itself, so it can be
/// used wherever a deck is expected.
///
/// ```
/// use deck_api::{Deck, DeckHandle};
///
/// fn geometry_of(handle: &DeckHandle) -> deck_api::Geometry {
///     handle.geometry()
/// }
/// ```
pub struct DeckHandle {
    inner: Box<dyn Deck>,
}

impl DeckHandle {
    /// Boxes a deck.
    ///
    /// ```
    /// use deck_api::{DeckHandle, Geometry};
    ///
    /// fn handle_of(deck: Box<dyn deck_api::Deck>) -> DeckHandle {
    ///     DeckHandle::new(deck)
    /// }
    /// ```
    pub fn new(inner: Box<dyn Deck>) -> Self {
        Self { inner }
    }
}

impl Deck for DeckHandle {
    fn geometry(&self) -> Geometry {
        self.inner.geometry()
    }

    fn image_spec(&self) -> ImageSpec {
        self.inner.image_spec()
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    fn identity(&self) -> Result<String, DeckError> {
        self.inner.identity()
    }

    fn set_brightness(&mut self, value: u8) -> Result<(), DeckError> {
        self.inner.set_brightness(value)
    }

    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
        self.inner.set_key_image(key, jpeg)
    }

    fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError> {
        self.inner.clear_key(key)
    }

    fn clear_all(&mut self) -> Result<(), DeckError> {
        self.inner.clear_all()
    }

    fn sleep(&mut self) -> Result<(), DeckError> {
        self.inner.sleep()
    }

    fn keep_alive(&mut self) -> Result<(), DeckError> {
        self.inner.keep_alive()
    }

    fn set_dial(&mut self, dial: u8, value: u8) -> Result<(), DeckError> {
        self.inner.set_dial(dial, value)
    }

    fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
        self.inner.poll_events(timeout)
    }

    fn reconnect(&mut self) -> Result<(), DeckError> {
        self.inner.reconnect()
    }
}

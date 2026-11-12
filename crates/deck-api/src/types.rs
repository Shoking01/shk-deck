//! The plain data a deck exposes about itself.
//!
//! Nothing here holds a handle, a clock or a device: these are the facts a
//! deck states once, so the code above the seam can adapt to the deck in
//! front of it instead of to the model it was written for.

use std::fmt;

/// A key on a deck, addressed by its position in the deck's own key order.
///
/// The index is **0-based**: it is the key's place in the deck's order, not a
/// number the user sees. The window shows 1-based numbers, so it adds one at
/// the boundary and the two never mix.
///
/// ```
/// use deck_api::KeyId;
///
/// assert_eq!(KeyId::new(0).index(), 0);
/// assert_eq!(KeyId::new(14).index(), 14);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(u16);

impl KeyId {
    /// The first key of any deck.
    pub const FIRST: KeyId = KeyId(0);

    /// Builds a key id from its 0-based index.
    ///
    /// ```
    /// use deck_api::KeyId;
    ///
    /// assert_eq!(KeyId::new(3).index(), 3);
    /// ```
    pub fn new(index: u16) -> Self {
        Self(index)
    }

    /// The 0-based index of this key.
    ///
    /// ```
    /// use deck_api::KeyId;
    ///
    /// assert_eq!(KeyId::new(7).index(), 7);
    /// ```
    pub fn index(self) -> u16 {
        self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The physical shape of a deck's keys.
///
/// The rows are numbered in the deck's own key order, so row 0 is the deck's
/// first row — which is the bottom row on a deck whose firmware counts from
/// the bottom. The geometry does not know about pixels or cases; it only says
/// how many keys there are and how they are arranged.
///
/// ```
/// use deck_api::{Geometry, KeyId};
///
/// let geometry = Geometry::new(6, 2, 3);
/// assert_eq!(geometry.key_count(), 6);
/// assert_eq!(geometry.position(KeyId::new(4)), Some((1, 1)));
/// assert_eq!(geometry.position(KeyId::new(6)), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    key_count: u16,
    rows: u16,
    columns: u16,
}

impl Geometry {
    /// Describes a deck with `key_count` keys laid out in `rows` rows of
    /// `columns` keys each.
    ///
    /// ```
    /// use deck_api::Geometry;
    ///
    /// let geometry = Geometry::new(8, 2, 4);
    /// assert_eq!(geometry.rows(), 2);
    /// assert_eq!(geometry.columns(), 4);
    /// ```
    pub fn new(key_count: u16, rows: u16, columns: u16) -> Self {
        Self {
            key_count,
            rows,
            columns,
        }
    }

    /// How many keys the deck has.
    ///
    /// ```
    /// use deck_api::Geometry;
    ///
    /// assert_eq!(Geometry::new(8, 2, 4).key_count(), 8);
    /// ```
    pub fn key_count(&self) -> u16 {
        self.key_count
    }

    /// How many rows of keys the deck has.
    ///
    /// ```
    /// use deck_api::Geometry;
    ///
    /// assert_eq!(Geometry::new(8, 2, 4).rows(), 2);
    /// ```
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// How many keys each row has.
    ///
    /// ```
    /// use deck_api::Geometry;
    ///
    /// assert_eq!(Geometry::new(8, 2, 4).columns(), 4);
    /// ```
    pub fn columns(&self) -> u16 {
        self.columns
    }

    /// Whether `key` exists on this deck.
    ///
    /// ```
    /// use deck_api::{Geometry, KeyId};
    ///
    /// let geometry = Geometry::new(6, 2, 3);
    /// assert!(geometry.contains(KeyId::new(5)));
    /// assert!(!geometry.contains(KeyId::new(6)));
    /// ```
    pub fn contains(&self, key: KeyId) -> bool {
        key.index() < self.key_count
    }

    /// The `(row, column)` of `key`, or `None` when the key is not on this
    /// deck.
    ///
    /// ```
    /// use deck_api::{Geometry, KeyId};
    ///
    /// let geometry = Geometry::new(6, 2, 3);
    /// assert_eq!(geometry.position(KeyId::new(0)), Some((0, 0)));
    /// assert_eq!(geometry.position(KeyId::new(5)), Some((1, 2)));
    /// ```
    pub fn position(&self, key: KeyId) -> Option<(u16, u16)> {
        if !self.contains(key) {
            return None;
        }
        // A deck that declares no columns still has keys; treating the row as
        // a single line keeps the arithmetic honest instead of dividing by zero.
        let columns = self.columns.max(1);
        let index = key.index();
        Some((index / columns, index % columns))
    }
}

/// The encoding a deck accepts for the images on its keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// A JPEG image.
    Jpeg,
}

/// The shape of the image a deck accepts for one key.
///
/// The payload limit is part of the contract, not a detail of one model: a
/// deck that takes smaller images refuses a bigger one with a typed error
/// instead of truncating it.
///
/// ```
/// use deck_api::{ImageFormat, ImageSpec};
///
/// let spec = ImageSpec::new(72, 72, 4_096, ImageFormat::Jpeg);
/// assert_eq!(spec.width(), 72);
/// assert_eq!(spec.max_bytes(), 4_096);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSpec {
    width: u16,
    height: u16,
    max_bytes: usize,
    format: ImageFormat,
}

impl ImageSpec {
    /// Describes the images this deck accepts.
    ///
    /// ```
    /// use deck_api::{ImageFormat, ImageSpec};
    ///
    /// let spec = ImageSpec::new(100, 100, 9_000, ImageFormat::Jpeg);
    /// assert_eq!(spec.height(), 100);
    /// ```
    pub fn new(width: u16, height: u16, max_bytes: usize, format: ImageFormat) -> Self {
        Self {
            width,
            height,
            max_bytes,
            format,
        }
    }

    /// The image width in pixels.
    ///
    /// ```
    /// use deck_api::{ImageFormat, ImageSpec};
    ///
    /// assert_eq!(ImageSpec::new(100, 100, 9_000, ImageFormat::Jpeg).width(), 100);
    /// ```
    pub fn width(&self) -> u16 {
        self.width
    }

    /// The image height in pixels.
    ///
    /// ```
    /// use deck_api::{ImageFormat, ImageSpec};
    ///
    /// assert_eq!(ImageSpec::new(100, 100, 9_000, ImageFormat::Jpeg).height(), 100);
    /// ```
    pub fn height(&self) -> u16 {
        self.height
    }

    /// The largest image the deck accepts, in bytes.
    ///
    /// ```
    /// use deck_api::{ImageFormat, ImageSpec};
    ///
    /// assert_eq!(ImageSpec::new(72, 72, 4_096, ImageFormat::Jpeg).max_bytes(), 4_096);
    /// ```
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// The encoding the deck accepts.
    ///
    /// ```
    /// use deck_api::{ImageFormat, ImageSpec};
    ///
    /// assert_eq!(
    ///     ImageSpec::new(72, 72, 4_096, ImageFormat::Jpeg).format(),
    ///     ImageFormat::Jpeg
    /// );
    /// ```
    pub fn format(&self) -> ImageFormat {
        self.format
    }
}

/// What a deck can do, stated as data.
///
/// The point of this type is that a caller asks before it acts, and an
/// operation the deck does not have fails with a typed error rather than
/// doing nothing.
///
/// ```
/// use deck_api::Capabilities;
///
/// let capabilities = Capabilities::new(true, false, true, false);
/// assert!(capabilities.brightness());
/// assert!(!capabilities.clear_key());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    brightness: bool,
    clear_key: bool,
    clear_all: bool,
    sleep: bool,
}

impl Capabilities {
    /// A deck that can do none of the optional things.
    pub const NONE: Capabilities = Capabilities {
        brightness: false,
        clear_key: false,
        clear_all: false,
        sleep: false,
    };

    /// States what the deck can do.
    ///
    /// ```
    /// use deck_api::Capabilities;
    ///
    /// let capabilities = Capabilities::new(true, true, true, false);
    /// assert!(capabilities.sleep() == false);
    /// ```
    pub fn new(brightness: bool, clear_key: bool, clear_all: bool, sleep: bool) -> Self {
        Self {
            brightness,
            clear_key,
            clear_all,
            sleep,
        }
    }

    /// Whether the deck's brightness can be set.
    ///
    /// ```
    /// use deck_api::Capabilities;
    ///
    /// assert!(Capabilities::new(true, false, false, false).brightness());
    /// ```
    pub fn brightness(&self) -> bool {
        self.brightness
    }

    /// Whether one key can be cleared on its own.
    ///
    /// ```
    /// use deck_api::Capabilities;
    ///
    /// assert!(!Capabilities::new(true, false, false, false).clear_key());
    /// ```
    pub fn clear_key(&self) -> bool {
        self.clear_key
    }

    /// Whether every key can be cleared at once.
    ///
    /// ```
    /// use deck_api::Capabilities;
    ///
    /// assert!(Capabilities::new(true, false, true, false).clear_all());
    /// ```
    pub fn clear_all(&self) -> bool {
        self.clear_all
    }

    /// Whether the deck can be told to sleep.
    ///
    /// ```
    /// use deck_api::Capabilities;
    ///
    /// assert!(!Capabilities::NONE.sleep());
    /// ```
    pub fn sleep(&self) -> bool {
        self.sleep
    }
}

/// Whether a key is down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    /// The key went down.
    Pressed,
    /// The key came back up.
    Released,
}

/// One key transition reported by a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    key: KeyId,
    state: ButtonState,
}

impl KeyEvent {
    /// Builds an event for one key.
    ///
    /// ```
    /// use deck_api::{ButtonState, KeyEvent, KeyId};
    ///
    /// let event = KeyEvent::new(KeyId::new(2), ButtonState::Pressed);
    /// assert_eq!(event.key().index(), 2);
    /// ```
    pub fn new(key: KeyId, state: ButtonState) -> Self {
        Self { key, state }
    }

    /// The key this event is about.
    ///
    /// ```
    /// use deck_api::{ButtonState, KeyEvent, KeyId};
    ///
    /// let event = KeyEvent::new(KeyId::new(0), ButtonState::Released);
    /// assert_eq!(event.state(), ButtonState::Released);
    /// ```
    pub fn key(&self) -> KeyId {
        self.key
    }

    /// Whether the key went down or came up.
    ///
    /// ```
    /// use deck_api::{ButtonState, KeyEvent, KeyId};
    ///
    /// let event = KeyEvent::new(KeyId::new(0), ButtonState::Pressed);
    /// assert_eq!(event.state(), ButtonState::Pressed);
    /// ```
    pub fn state(&self) -> ButtonState {
        self.state
    }
}

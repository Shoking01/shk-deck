//! Physical key indices and mappings.
//!
//! The SS550 v3 has a 3×5 grid (15 keys). OpenDeck's physical layout has
//! bottom-left as (0,0); this crate uses the same mapping.
//!
//! ## Key Mapping Table
//!
//! Positions are `(row, column)` with **row 0 = bottom**, column 0 = left.
//! The flat index is `row * 5 + column`, which is also the OpenDeck key.
//!
//! | OpenDeck key | Position | Firmware key |
//! |--------------|----------|--------------|
//! | 0 (BottomLeft) | (0,0) | 1 |
//! | 1 (Bottom2) | (0,1) | 2 |
//! | 2 (BottomCenter) | (0,2) | 3 |
//! | 3 (Bottom3) | (0,3) | 4 |
//! | 4 (BottomRight) | (0,4) | 5 |
//! | 5 (Left) | (1,0) | 6 |
//! | 6 (Middle2) | (1,1) | 7 |
//! | 7 (Center) | (1,2) | 8 |
//! | 8 (Middle3) | (1,3) | 9 |
//! | 9 (Right) | (1,4) | 10 |
//! | 10 (TopLeft) | (2,0) | 11 |
//! | 11 (Top2) | (2,1) | 12 |
//! | 12 (TopCenter) | (2,2) | 13 |
//! | 13 (Top3) | (2,3) | 14 |
//! | 14 (TopRight) | (2,4) | 15 |
//!
//! Firmware keys are 1-based (1-15). OpenDeck keys are 0-based (0-14).
//!
//! **Reports use a third numbering**: the key index in an input report is
//! row-major from the **top**-left, so it is neither of the two above.
//! `ss550_protocol::read_index_to_firmware_key` applies that row flip.

/// Number of keys on the SS550 v3.
pub const KEY_COUNT: usize = 15;

/// Physical key positions (row, column), bottom-left = (0,0).
/// Flattened to a single index: `key = row * 5 + col`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Bottom-left position (row 2, col 0).
    BottomLeft = 0,
    /// Bottom row second from left (row 2, col 1).
    Bottom2 = 1,
    /// Bottom-center (row 2, col 2).
    BottomCenter = 2,
    /// Bottom row second from right (row 2, col 3).
    Bottom3 = 3,
    /// Bottom-right (row 2, col 4).
    BottomRight = 4,
    /// Middle-left (row 1, col 0).
    Left = 5,
    /// Middle row second from left (row 1, col 1).
    Middle2 = 6,
    /// Center of the device (row 1, col 2).
    Center = 7,
    /// Middle row second from right (row 1, col 3).
    Middle3 = 8,
    /// Middle-right (row 1, col 4).
    Right = 9,
    /// Top-left (row 0, col 0).
    TopLeft = 10,
    /// Top row second from left (row 0, col 1).
    Top2 = 11,
    /// Top-center (row 0, col 2).
    TopCenter = 12,
    /// Top row second from right (row 0, col 3).
    Top3 = 13,
    /// Top-right (row 0, col 4).
    TopRight = 14,
}

impl Key {
    /// Convert from u8 index (0-14) to Key enum.
    pub fn from_index(idx: u8) -> Result<Self, crate::Ss550Error> {
        match idx {
            0 => Ok(Key::BottomLeft),
            1 => Ok(Key::Bottom2),
            2 => Ok(Key::BottomCenter),
            3 => Ok(Key::Bottom3),
            4 => Ok(Key::BottomRight),
            5 => Ok(Key::Left),
            6 => Ok(Key::Middle2),
            7 => Ok(Key::Center),
            8 => Ok(Key::Middle3),
            9 => Ok(Key::Right),
            10 => Ok(Key::TopLeft),
            11 => Ok(Key::Top2),
            12 => Ok(Key::TopCenter),
            13 => Ok(Key::Top3),
            14 => Ok(Key::TopRight),
            _ => Err(crate::Ss550Error::KeyOutOfRange(idx)),
        }
    }

    /// Convert Key enum to u8 index.
    pub fn to_index(self) -> u8 {
        self as u8
    }

    /// Converts a 1-based firmware key (1-15) to its `Key`.
    ///
    /// Returns `None` outside `1..=15`.
    pub fn from_firmware_key(fw_key: u8) -> Option<Self> {
        let index = firmware_to_open_deck(fw_key)?;
        Self::from_index(index).ok()
    }

    /// Position as `(row, column)` for image placement.
    ///
    /// **Row 0 is the bottom row**, matching the enum order and the OpenDeck
    /// numbering: `BottomLeft` is `(0, 0)` and `TopRight` is `(2, 4)`.
    pub fn position(self) -> (usize, usize) {
        (self as usize / 5, self as usize % 5)
    }
}

impl TryFrom<u8> for Key {
    type Error = crate::Ss550Error;

    fn try_from(idx: u8) -> Result<Self, Self::Error> {
        Self::from_index(idx)
    }
}

impl From<Key> for u8 {
    fn from(key: Key) -> Self {
        key.to_index()
    }
}

// -----------------------------------------------------------------------------
// Firmware key mapping
// -----------------------------------------------------------------------------

/// Maps an OpenDeck logical key (0-14) to the firmware's 1-based key index (1-15).
/// The mapping is linear: OpenDeck key N → Firmware key N+1.
const OPENDECK_TO_FIRMWARE: [u8; KEY_COUNT] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// Converts an OpenDeck logical key to the firmware's 1-based key index.
pub fn open_deck_to_firmware(key: u8) -> u8 {
    if (key as usize) < KEY_COUNT {
        OPENDECK_TO_FIRMWARE[key as usize]
    } else {
        // Out of range: return a safe default (15)
        15
    }
}

/// Converts a firmware 1-based key index back to OpenDeck logical index (0-14).
#[allow(dead_code)]
pub fn firmware_to_open_deck(fw_key: u8) -> Option<u8> {
    (1..=15).contains(&fw_key).then_some(fw_key - 1)
}

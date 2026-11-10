//! The deck keys, addressed the way the user sees them.

use serde::{Deserialize, Serialize};
use ss550_driver::Key;

use crate::ActionError;

/// A deck key, addressed by the **1-based number shown in the UI**.
///
/// Key 1 is the bottom-left key and key 15 the top-right one, which is also
/// the firmware's own numbering: the same number appears in an input report
/// and in a paint command, so one number identifies a key everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8")]
pub struct KeyNumber(u8);

impl KeyNumber {
    /// The lowest valid key number.
    pub const MIN: u8 = 1;

    /// The highest valid key number.
    pub const MAX: u8 = 15;

    /// Builds a key number, rejecting anything outside `1..=15`.
    ///
    /// ```
    /// use ss550_engine::KeyNumber;
    ///
    /// assert_eq!(KeyNumber::new(1).expect("valid").get(), 1);
    /// assert!(KeyNumber::new(0).is_err());
    /// assert!(KeyNumber::new(16).is_err());
    /// ```
    pub fn new(number: u8) -> Result<Self, ActionError> {
        if (Self::MIN..=Self::MAX).contains(&number) {
            Ok(Self(number))
        } else {
            Err(ActionError::KeyOutOfRange(number))
        }
    }

    /// The 1-based number of this key.
    pub fn get(self) -> u8 {
        self.0
    }

    /// The physical key this number refers to.
    pub fn key(self) -> Key {
        match Key::from_index(self.0 - 1) {
            Ok(key) => key,
            // Unreachable: a `KeyNumber` only exists for 1..=15.
            Err(_) => Key::BottomLeft,
        }
    }

    /// The number of a physical key.
    pub fn from_key(key: Key) -> Self {
        Self(key.to_index() + 1)
    }
}

/// The key the UI starts on: the bottom-left one.
///
/// There is deliberately no `#[derive(Default)]` here: the derived value would
/// be key `0`, which does not exist on the deck.
impl Default for KeyNumber {
    fn default() -> Self {
        Self(Self::MIN)
    }
}

impl TryFrom<u8> for KeyNumber {
    type Error = ActionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<KeyNumber> for u8 {
    fn from(value: KeyNumber) -> u8 {
        value.0
    }
}

impl std::fmt::Display for KeyNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_numbers_outside_the_deck() {
        assert!(KeyNumber::new(0).is_err());
        assert!(KeyNumber::new(16).is_err());
        assert!(KeyNumber::new(255).is_err());
        assert!(KeyNumber::new(1).is_ok());
        assert!(KeyNumber::new(15).is_ok());
    }

    #[test]
    fn the_default_key_is_the_bottom_left_one() {
        assert_eq!(KeyNumber::default().get(), 1);
        assert_eq!(KeyNumber::default().key(), Key::BottomLeft);
    }

    #[test]
    fn every_physical_key_maps_to_a_distinct_number() {
        let mut seen = [false; 16];
        for index in 0..15u8 {
            let key = Key::from_index(index).expect("in range");
            let number = KeyNumber::from_key(key);
            assert!(!seen[number.get() as usize], "key {index} collides");
            seen[number.get() as usize] = true;
            assert_eq!(number.key(), key, "round trip failed for {index}");
        }
        assert!(seen[1..].iter().all(|&used| used), "every key reachable");
    }

    #[test]
    fn the_bottom_left_key_is_one_and_the_top_right_is_fifteen() {
        assert_eq!(KeyNumber::from_key(Key::BottomLeft).get(), 1);
        assert_eq!(KeyNumber::from_key(Key::TopRight).get(), 15);
        assert_eq!(KeyNumber::new(1).expect("valid").key(), Key::BottomLeft);
        assert_eq!(KeyNumber::new(15).expect("valid").key(), Key::TopRight);
    }
}

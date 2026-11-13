//! The deck keys, addressed the way the user sees them.

use serde::{Deserialize, Serialize};

use crate::ActionError;

/// A deck key, addressed by the **1-based number shown in the UI**.
///
/// Key 1 is the first key in the deck's own order and the highest number is the
/// deck's key count. The number is the whole identity: the engine does not know
/// which device the keys belong to, so it cannot turn a number into a physical
/// key, and it does not guess one when the number is out of range — a key that
/// does not exist is an error, never a silent fallback to another key.
///
/// A **number** is valid as long as it is at least 1. Whether that key exists is
/// a question only the deck can answer, with its geometry, so the engine keeps
/// no upper bound of its own: a window driving a six-key deck and a window
/// driving a larger one use the same engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8")]
pub struct KeyNumber(u8);

impl KeyNumber {
    /// The lowest valid key number.
    pub const MIN: u8 = 1;

    /// Builds a key number, rejecting zero.
    ///
    /// ```
    /// use ss550_engine::KeyNumber;
    ///
    /// assert_eq!(KeyNumber::new(1).expect("valid").get(), 1);
    /// assert!(KeyNumber::new(0).is_err());
    /// assert!(KeyNumber::new(200).is_ok());
    /// ```
    pub fn new(number: u8) -> Result<Self, ActionError> {
        if number >= Self::MIN {
            Ok(Self(number))
        } else {
            Err(ActionError::KeyOutOfRange(number))
        }
    }

    /// The 1-based number of this key.
    pub fn get(self) -> u8 {
        self.0
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
        assert!(KeyNumber::new(1).is_ok());
        assert!(KeyNumber::new(15).is_ok());
        assert!(KeyNumber::new(16).is_ok());
        assert!(KeyNumber::new(255).is_ok());
    }

    #[test]
    fn the_default_key_is_the_first_one() {
        assert_eq!(KeyNumber::default().get(), 1);
    }

    #[test]
    fn the_number_is_the_one_the_ui_shows() {
        assert_eq!(KeyNumber::new(1).expect("valid").get(), 1);
        assert_eq!(KeyNumber::new(15).expect("valid").get(), 15);
        assert_eq!(u8::from(KeyNumber::new(7).expect("valid")), 7);
        assert_eq!(KeyNumber::new(7).expect("valid").to_string(), "#7");
    }
}

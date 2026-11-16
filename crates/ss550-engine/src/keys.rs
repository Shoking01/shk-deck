//! Key combinations: the keystrokes a key can send to the focused window.
//!
//! A combination is written as its parts joined by `+`: the modifiers and then
//! exactly one base key. The parts may arrive in any order and in any case, and
//! they are stored in one canonical spelling, so two combinations that mean the
//! same thing are equal and are written the same way. `SHIFT+ctrl+M`,
//! `ctrl+shift+m` and `m+ctrl+shift` all become `ctrl+shift+m`, which is what
//! the bindings file keeps.
//!
//! ## The vocabulary
//!
//! | Part | Accepted |
//! | --- | --- |
//! | Modifiers | `ctrl` (`control`), `alt`, `shift`, `win` (`super`) |
//! | Letters | `a`–`z` |
//! | Digits | `0`–`9` |
//! | Function keys | `f1`–`f24` |
//! | Arrows | `up`, `down`, `left`, `right` |
//! | Named keys | `enter`, `escape`, `tab`, `space`, `backspace`, `delete`, `home`, `end`, `pageup`, `pagedown`, `insert` |
//! | Punctuation | `,` `.` `/` `;` `'` `[` `]` `\` `-` `=` `` ` `` |
//!
//! `+` is the separator, not a key: to send it, hold `shift` and press `=`.
//! Anything outside this table is an error with a name, never a guess — the
//! engine cannot ask the keyboard what it has, and a key it invented would be
//! a keystroke the user never asked for.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::KeyComboError;

/// The punctuation keys this app can send.
///
/// These are the unshifted keys a full keyboard already has, minus `+`, which is
/// the separator between the parts of a combination.
const PUNCTUATION: [char; 11] = [',', '.', '/', ';', '\'', '[', ']', '\\', '-', '=', '`'];

/// The longest a combination can read once it is written out for a key tile.
///
/// The bound is not a tuning knob: four modifiers and the longest key name
/// cannot produce more than this, so a tile never has to cut a combination in
/// half. A real combination is far shorter — `Ctrl+Shift+M` is the usual shape.
pub const MAX_SUMMARY_CHARS: usize = 28;

/// A modifier that is held down while the base key is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    /// `ctrl`
    Ctrl,
    /// `alt`
    Alt,
    /// `shift`
    Shift,
    /// `win`, the Windows key. `super` is accepted as the same modifier.
    Win,
}

impl Modifier {
    /// The name this modifier takes in the canonical string and in the file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::Alt => "alt",
            Self::Shift => "shift",
            Self::Win => "win",
        }
    }

    /// Reads one modifier name. The name is already lowercase and trimmed.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "ctrl" | "control" => Some(Self::Ctrl),
            "alt" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "win" | "super" => Some(Self::Win),
            _ => None,
        }
    }
}

impl fmt::Display for Modifier {
    /// The form a key tile shows: `Ctrl`, `Alt`, `Shift`, `Win`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Win => "Win",
        })
    }
}

/// A key that is not a letter, a digit or punctuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NamedKey {
    /// `up`
    Up,
    /// `down`
    Down,
    /// `left`
    Left,
    /// `right`
    Right,
    /// `enter`
    Enter,
    /// `escape`
    Escape,
    /// `tab`
    Tab,
    /// `space`
    Space,
    /// `backspace`
    Backspace,
    /// `delete`
    Delete,
    /// `home`
    Home,
    /// `end`
    End,
    /// `pageup`
    PageUp,
    /// `pagedown`
    PageDown,
    /// `insert`
    Insert,
}

impl NamedKey {
    /// The name this key takes in the canonical string and in the file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Enter => "enter",
            Self::Escape => "escape",
            Self::Tab => "tab",
            Self::Space => "space",
            Self::Backspace => "backspace",
            Self::Delete => "delete",
            Self::Home => "home",
            Self::End => "end",
            Self::PageUp => "pageup",
            Self::PageDown => "pagedown",
            Self::Insert => "insert",
        }
    }

    /// Reads one named key. The name is already lowercase and trimmed.
    fn from_name(name: &str) -> Option<Self> {
        let key = match name {
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "enter" => Self::Enter,
            "escape" => Self::Escape,
            "tab" => Self::Tab,
            "space" => Self::Space,
            "backspace" => Self::Backspace,
            "delete" => Self::Delete,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" => Self::PageUp,
            "pagedown" => Self::PageDown,
            "insert" => Self::Insert,
            _ => return None,
        };
        Some(key)
    }
}

impl fmt::Display for NamedKey {
    /// The form a key tile shows: `Up`, `Enter`, `PageUp`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = self.name().chars();
        match chars.next() {
            Some(first) => {
                write!(f, "{}", first.to_ascii_uppercase())?;
                f.write_str(chars.as_str())
            }
            None => Ok(()),
        }
    }
}

/// The one non-modifier key a combination presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BaseKey {
    /// A letter, `a`–`z`, stored lowercase.
    Letter(char),
    /// A digit, `0`–`9`.
    Digit(char),
    /// A function key, `f1`–`f24`.
    Function(u8),
    /// A key with a name rather than a printed character.
    Named(NamedKey),
    /// A punctuation key, from the set this app documents.
    Punctuation(char),
}

impl BaseKey {
    /// The name this key takes in the canonical string and in the file.
    pub fn name(self) -> String {
        match self {
            Self::Letter(letter) | Self::Digit(letter) | Self::Punctuation(letter) => {
                letter.to_string()
            }
            Self::Function(number) => format!("f{number}"),
            Self::Named(key) => key.name().to_string(),
        }
    }

    /// Reads one key from a lowercase, trimmed token.
    fn parse(token: &str) -> Option<Self> {
        let single = {
            let mut chars = token.chars();
            match (chars.next(), chars.next()) {
                (Some(only), None) => Some(only),
                _ => None,
            }
        };
        if let Some(only) = single {
            return match only {
                'a'..='z' => Some(Self::Letter(only)),
                '0'..='9' => Some(Self::Digit(only)),
                other if PUNCTUATION.contains(&other) => Some(Self::Punctuation(other)),
                _ => None,
            };
        }

        if let Some(digits) = token.strip_prefix('f') {
            if let Ok(number) = digits.parse::<u8>() {
                if (1..=24).contains(&number) {
                    return Some(Self::Function(number));
                }
            }
        }

        NamedKey::from_name(token).map(Self::Named)
    }
}

impl fmt::Display for BaseKey {
    /// The form a key tile shows: `M`, `7`, `F24`, `Enter`, `,`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Letter(letter) => write!(f, "{}", letter.to_ascii_uppercase()),
            Self::Digit(digit) => write!(f, "{digit}"),
            Self::Function(number) => write!(f, "F{number}"),
            Self::Named(key) => write!(f, "{key}"),
            Self::Punctuation(mark) => write!(f, "{mark}"),
        }
    }
}

/// A key combination: modifiers held down while one base key is pressed.
///
/// A value of this type is **proof** that the combination is one this app can
/// send. The only ways to make one are [`KeyCombo::parse`], [`FromStr`] and
/// deserialising, and all three refuse anything outside the
/// [module vocabulary](self); there is no public constructor that takes the
/// parts, so an invalid combination cannot exist to be sent.
///
/// ```
/// use ss550_engine::KeyCombo;
///
/// let combo = KeyCombo::parse("Shift+Ctrl+M").expect("valid combination");
/// assert_eq!(combo.to_string(), "Ctrl+Shift+M");
/// assert_eq!(combo.canonical(), "ctrl+shift+m");
///
/// assert!(KeyCombo::parse("ctrl+shift").is_err(), "no base key");
/// assert!(KeyCombo::parse("ctrl+m+n").is_err(), "two base keys");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyCombo {
    /// The modifiers, in the canonical order and without repeats.
    modifiers: Vec<Modifier>,
    /// The one key the combination presses.
    key: BaseKey,
}

impl KeyCombo {
    /// Reads a combination such as `Ctrl+Shift+M`.
    ///
    /// The parts may be in any order and in any case. Anything the
    /// [module vocabulary](self) does not name is refused with an error that
    /// says which part is wrong and why.
    pub fn parse(text: &str) -> Result<Self, KeyComboError> {
        let combo = text.trim().to_string();
        if combo.is_empty() {
            return Err(KeyComboError::Empty);
        }

        let mut modifiers: Vec<Modifier> = Vec::new();
        let mut key: Option<BaseKey> = None;

        for step in text.split('+') {
            let step = step.trim().to_ascii_lowercase();
            if step.is_empty() {
                return Err(KeyComboError::EmptyStep { combo });
            }

            if let Some(modifier) = Modifier::from_name(&step) {
                if modifiers.contains(&modifier) {
                    return Err(KeyComboError::DuplicateModifier {
                        combo,
                        modifier: modifier.to_string(),
                    });
                }
                modifiers.push(modifier);
                continue;
            }

            match key {
                // Before the key, a token that is not a modifier is the key.
                None => match BaseKey::parse(&step) {
                    Some(parsed) => key = Some(parsed),
                    None => return Err(KeyComboError::UnknownKey { name: step }),
                },
                // After the key, a token that is a key is a second key, and
                // anything else can only have been meant as a modifier.
                Some(first) => match BaseKey::parse(&step) {
                    Some(second) => {
                        return Err(KeyComboError::TwoKeys {
                            combo,
                            first: first.to_string(),
                            second: second.to_string(),
                        })
                    }
                    None => return Err(KeyComboError::UnknownModifier { name: step }),
                },
            }
        }

        let Some(key) = key else {
            return Err(KeyComboError::NoKey { combo });
        };
        modifiers.sort();

        Ok(Self { modifiers, key })
    }

    /// The modifiers, in the canonical order and without repeats.
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    /// The one key the combination presses.
    pub fn base_key(&self) -> BaseKey {
        self.key
    }

    /// The spelling the bindings file keeps: lowercase, canonical order.
    ///
    /// ```
    /// use ss550_engine::KeyCombo;
    ///
    /// let combo = KeyCombo::parse("M+SHIFT+Ctrl").expect("valid combination");
    /// assert_eq!(combo.canonical(), "ctrl+shift+m");
    /// ```
    pub fn canonical(&self) -> String {
        let mut text = String::new();
        for modifier in &self.modifiers {
            text.push_str(modifier.name());
            text.push('+');
        }
        text.push_str(&self.key.name());
        text
    }
}

impl fmt::Display for KeyCombo {
    /// The form a key tile and the status line show: `Ctrl+Shift+M`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut written = false;
        for modifier in &self.modifiers {
            if written {
                f.write_str("+")?;
            }
            write!(f, "{modifier}")?;
            written = true;
        }
        if written {
            f.write_str("+")?;
        }
        write!(f, "{}", self.key)
    }
}

impl FromStr for KeyCombo {
    type Err = KeyComboError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl Serialize for KeyCombo {
    /// Writes the canonical spelling, so the file holds one form of each
    /// combination and a diff shows what actually changed.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.canonical())
    }
}

impl<'de> Deserialize<'de> for KeyCombo {
    /// Reads the canonical spelling, or any other spelling of the same
    /// combination. An unacceptable one fails with the parser's own message, so
    /// a hand-edited file names the part it got wrong.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_documented_part() {
        for text in [
            "m",
            "ctrl+m",
            "Ctrl+Shift+M",
            "alt+0",
            "ctrl+alt+shift+win+m",
            "super+m",
            "control+m",
            "f1",
            "f24",
            "ctrl+f12",
            "up",
            "ctrl+down",
            "shift+left",
            "alt+right",
            "enter",
            "escape",
            "tab",
            "space",
            "backspace",
            "delete",
            "home",
            "end",
            "pageup",
            "pagedown",
            "insert",
            "ctrl+,",
            "ctrl+.",
            "ctrl+/",
            "ctrl+;",
            "ctrl+'",
            "ctrl+[",
            "ctrl+]",
            "ctrl+\\",
            "ctrl+-",
            "ctrl+=",
            "ctrl+`",
        ] {
            assert!(KeyCombo::parse(text).is_ok(), "{text:?} should be accepted");
        }
    }

    #[test]
    fn order_and_case_do_not_change_what_a_combination_means() {
        let canonical = KeyCombo::parse("ctrl+shift+m").expect("valid combination");
        for text in [
            "Ctrl+Shift+M",
            "SHIFT+CTRL+M",
            "m+shift+ctrl",
            "  ctrl + shift + m  ",
        ] {
            let parsed = KeyCombo::parse(text).expect("valid combination");
            assert_eq!(parsed, canonical, "for {text:?}");
            assert_eq!(parsed.canonical(), "ctrl+shift+m", "for {text:?}");
        }
    }

    #[test]
    fn an_empty_combination_is_refused() {
        for text in ["", "   ", "\t"] {
            assert!(
                matches!(KeyCombo::parse(text), Err(KeyComboError::Empty)),
                "{text:?} should be refused as empty"
            );
        }
    }

    #[test]
    fn modifiers_alone_are_refused() {
        for text in ["ctrl", "ctrl+shift", "win+alt"] {
            assert!(
                matches!(KeyCombo::parse(text), Err(KeyComboError::NoKey { .. })),
                "{text:?} names no key"
            );
        }
    }

    #[test]
    fn two_keys_are_refused() {
        for text in ["m+n", "ctrl+m+n", "ctrl+m+enter", "a+1"] {
            let error = KeyCombo::parse(text).expect_err("two keys must be refused");
            assert!(
                matches!(error, KeyComboError::TwoKeys { .. }),
                "{text:?}: {error}"
            );
        }
    }

    #[test]
    fn an_unknown_modifier_is_named() {
        let error = KeyCombo::parse("ctrl+m+hyper").expect_err("unknown modifier");
        assert_eq!(
            error,
            KeyComboError::UnknownModifier {
                name: "hyper".to_string()
            }
        );
        assert!(error.to_string().contains("hyper"));
    }

    #[test]
    fn an_unknown_key_is_named() {
        for (text, name) in [
            ("hyper+m", "hyper"),
            ("ctrl+hyper", "hyper"),
            ("f25", "f25"),
            ("f0", "f0"),
            ("ctrl+f25", "f25"),
            ("é", "é"),
            ("ctrl+shift+é", "é"),
        ] {
            let error = KeyCombo::parse(text).expect_err("unknown key");
            assert!(
                error.to_string().contains(name),
                "{text:?}: the message must name {name:?}: {error}"
            );
        }
    }

    #[test]
    fn a_repeated_modifier_is_refused() {
        let error = KeyCombo::parse("ctrl+ctrl+m").expect_err("a repeat is a typo");
        assert_eq!(
            error,
            KeyComboError::DuplicateModifier {
                combo: "ctrl+ctrl+m".to_string(),
                modifier: "Ctrl".to_string()
            }
        );
    }

    #[test]
    fn an_empty_step_is_refused() {
        for text in ["+", "+m", "m+", "ctrl++m"] {
            let error = KeyCombo::parse(text).expect_err("empty step");
            assert!(
                matches!(error, KeyComboError::EmptyStep { .. }),
                "{text:?}: {error}"
            );
        }
    }

    #[test]
    fn a_combination_writes_itself_for_a_key_tile() {
        let combo = KeyCombo::parse("ctrl+shift+m").expect("valid combination");
        assert_eq!(combo.to_string(), "Ctrl+Shift+M");
        assert_eq!(combo.modifiers().len(), 2);
        assert_eq!(combo.base_key(), BaseKey::Letter('m'));

        let keys = KeyCombo::parse("win+f24").expect("valid combination");
        assert_eq!(keys.to_string(), "Win+F24");

        let punctuation = KeyCombo::parse("ctrl+,").expect("valid combination");
        assert_eq!(punctuation.to_string(), "Ctrl+,");
    }

    #[test]
    fn the_longest_combination_fits_a_key_tile() {
        let longest = KeyCombo::parse("ctrl+alt+shift+win+backspace").expect("valid combination");
        assert_eq!(longest.to_string(), "Ctrl+Alt+Shift+Win+Backspace");
        assert_eq!(longest.to_string().chars().count(), MAX_SUMMARY_CHARS);
    }

    #[test]
    fn a_combination_round_trips_as_a_canonical_string() {
        let combo = KeyCombo::parse("Shift+Ctrl+M").expect("valid combination");
        let json = serde_json::to_string(&combo).expect("serialise");
        assert_eq!(json, r#""ctrl+shift+m""#);
        let restored: KeyCombo = serde_json::from_str(&json).expect("parse");
        assert_eq!(restored, combo);

        // Any spelling of the same combination loads as the canonical one.
        let spelled: KeyCombo = serde_json::from_str(r#""M+Ctrl+Shift""#).expect("parse");
        assert_eq!(spelled, combo);
    }

    #[test]
    fn a_combination_that_does_not_exist_fails_to_load_with_the_reason() {
        let error = serde_json::from_str::<KeyCombo>(r#""ctrl+shift""#).expect_err("must fail");
        assert!(
            error.to_string().contains("no key"),
            "the parse message must survive: {error}"
        );
    }
}

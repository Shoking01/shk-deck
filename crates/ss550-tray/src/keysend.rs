//! Sending key combinations with Win32 `SendInput`.
//!
//! A combination is delivered the way a hand would type it: the modifiers go
//! down in a fixed order, the key goes down and comes back up, and the
//! modifiers are released in the reverse order. The order is fixed on purpose.
//! `Ctrl`, `Alt`, `Shift`, `Win` is the order the system itself reports, and
//! releasing in the reverse of the presses is what leaves no modifier stuck
//! down when a send fails halfway.
//!
//! Everything except the `SendInput` call is a plain function over plain data,
//! so the mapping is tested without a keyboard: the codes below are the ones
//! the platform documents, pinned by the tests in this module.
//!
//! ## The mapping
//!
//! | Engine | Virtual-key |
//! | --- | --- |
//! | `Modifier::Ctrl` | `VK_CONTROL` `0x11` |
//! | `Modifier::Alt` | `VK_MENU` `0x12` |
//! | `Modifier::Shift` | `VK_SHIFT` `0x10` |
//! | `Modifier::Win` | `VK_LWIN` `0x5B` |
//! | `BaseKey::Letter(a..z)` | `VK_A`..`VK_Z`, `0x41`..`0x5A` |
//! | `BaseKey::Digit(0..9)` | `VK_0`..`VK_9`, `0x30`..`0x39` |
//! | `BaseKey::Function(1..24)` | `VK_F1`..`VK_F24`, `0x70`..`0x87` |
//! | `BaseKey::Named(..)` | the table in [`named_code`] |
//! | `BaseKey::Punctuation(..)` | the OEM keys in [`punctuation_code`] |

use ss550_engine::{BaseKey, KeyCombo, KeySender, MediaCommand, Modifier, NamedKey};

/// `VK_CONTROL`: the left and right Ctrl keys together.
const VK_CONTROL: u16 = 0x11;
/// `VK_MENU`: the Alt key.
const VK_MENU: u16 = 0x12;
/// `VK_SHIFT`: the left and right Shift keys together.
const VK_SHIFT: u16 = 0x10;
/// `VK_LWIN`: the left Windows key.
const VK_LWIN: u16 = 0x5B;

/// `VK_F1`, the first function key. `F24` is this plus 23.
const VK_F1: u16 = 0x70;

/// The order the modifiers are pressed in; the reverse order releases them.
///
/// It is stated here rather than read off the engine's own ordering: the
/// sequence that goes to the system is this crate's promise, and a change to
/// the engine's `Ord` must not silently reorder a keystroke.
const MODIFIER_ORDER: [Modifier; 4] = [
    Modifier::Ctrl,
    Modifier::Alt,
    Modifier::Shift,
    Modifier::Win,
];

/// One key event: a virtual-key code and whether it is a release.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct KeyStroke {
    /// The virtual-key code the event carries.
    code: u16,
    /// True when the key is released.
    up: bool,
}

/// The virtual-key code of one modifier.
fn modifier_code(modifier: Modifier) -> u16 {
    match modifier {
        Modifier::Ctrl => VK_CONTROL,
        Modifier::Alt => VK_MENU,
        Modifier::Shift => VK_SHIFT,
        Modifier::Win => VK_LWIN,
    }
}

/// The virtual-key code of the key a combination presses.
///
/// The engine's vocabulary is closed, so every key it can build has a code
/// here. Anything else is refused with a sentence rather than guessed at: a
/// made-up code would press a key the user never asked for.
fn base_code(key: BaseKey) -> Result<u16, String> {
    Ok(match key {
        // `VK_A`..`VK_Z` are the ASCII capitals, in order.
        BaseKey::Letter(letter) => u16::from(letter.to_ascii_uppercase() as u8),
        // `VK_0`..`VK_9` are the ASCII digits, in order.
        BaseKey::Digit(digit) => u16::from(digit as u8),
        BaseKey::Function(number) => VK_F1 + u16::from(number) - 1,
        BaseKey::Named(named) => named_code(named),
        BaseKey::Punctuation(mark) => punctuation_code(mark)?,
    })
}

/// The virtual-key code of a key that has a name rather than a character.
fn named_code(key: NamedKey) -> u16 {
    match key {
        NamedKey::Up => 0x26,        // VK_UP
        NamedKey::Down => 0x28,      // VK_DOWN
        NamedKey::Left => 0x25,      // VK_LEFT
        NamedKey::Right => 0x27,     // VK_RIGHT
        NamedKey::Enter => 0x0D,     // VK_RETURN
        NamedKey::Escape => 0x1B,    // VK_ESCAPE
        NamedKey::Tab => 0x09,       // VK_TAB
        NamedKey::Space => 0x20,     // VK_SPACE
        NamedKey::Backspace => 0x08, // VK_BACK
        NamedKey::Delete => 0x2E,    // VK_DELETE
        NamedKey::Home => 0x24,      // VK_HOME
        NamedKey::End => 0x23,       // VK_END
        NamedKey::PageUp => 0x21,    // VK_PRIOR
        NamedKey::PageDown => 0x22,  // VK_NEXT
        NamedKey::Insert => 0x2D,    // VK_INSERT
    }
}

/// The virtual-key code of one of the punctuation keys the engine accepts.
///
/// These are the OEM keys: the codes follow the US layout's positions, which is
/// what `SendInput` addresses. A mark outside the engine's documented set has
/// no code and is refused.
fn punctuation_code(mark: char) -> Result<u16, String> {
    Ok(match mark {
        ',' => 0xBC,  // VK_OEM_COMMA
        '.' => 0xBE,  // VK_OEM_PERIOD
        '/' => 0xBF,  // VK_OEM_2
        ';' => 0xBA,  // VK_OEM_1
        '\'' => 0xDE, // VK_OEM_7
        '[' => 0xDB,  // VK_OEM_4
        ']' => 0xDD,  // VK_OEM_6
        '\\' => 0xDC, // VK_OEM_5
        '-' => 0xBD,  // VK_OEM_MINUS
        '=' => 0xBB,  // VK_OEM_PLUS
        '`' => 0xC0,  // VK_OEM_3
        other => return Err(format!("{other:?} has no virtual-key code")),
    })
}

/// The keystrokes one combination is made of, in the order they go out.
///
/// Modifiers down, the key down, the key up, modifiers up in reverse. The
/// result is what the platform layer turns into `INPUT` records; keeping it as
/// plain data is what lets the sequence be asserted in a test.
fn strokes(combo: &KeyCombo) -> Result<Vec<KeyStroke>, String> {
    let held: Vec<u16> = MODIFIER_ORDER
        .iter()
        .filter(|modifier| combo.modifiers().contains(modifier))
        .map(|modifier| modifier_code(*modifier))
        .collect();
    let key = base_code(combo.base_key())?;

    let mut strokes = Vec::with_capacity(held.len() * 2 + 2);
    for code in &held {
        strokes.push(KeyStroke {
            code: *code,
            up: false,
        });
    }
    strokes.push(KeyStroke {
        code: key,
        up: false,
    });
    strokes.push(KeyStroke {
        code: key,
        up: true,
    });
    for code in held.iter().rev() {
        strokes.push(KeyStroke {
            code: *code,
            up: true,
        });
    }
    Ok(strokes)
}

/// The platform's way of sending a keystroke to the focused window.
///
/// On Windows this is `SendInput`. Everywhere else there is no call this crate
/// is allowed to make, so a send reports that instead of pretending it worked:
/// the engine turns the sentence into a failure the window shows.
///
/// # Example
///
/// ```
/// use ss550_engine::{KeyCombo, KeySender};
///
/// // Constructing the sender never touches the keyboard; only `send` does.
/// let sender = ss550_tray::SystemKeySender;
/// let combo = KeyCombo::parse("ctrl+shift+m").expect("valid combination");
/// let _ = sender.send(&combo);
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemKeySender;

impl KeySender for SystemKeySender {
    fn send(&self, combo: &KeyCombo) -> Result<(), String> {
        let sequence = strokes(combo)?;
        deliver(&sequence)
    }

    /// Sends one media command as the key Windows gives every player.
    ///
    /// `VK_MEDIA_PLAY_PAUSE` and its three siblings are the virtual keys the
    /// system delivers to the **active media session**, which is why this needs no
    /// player to be named and works for Spotify, a browser tab and the system's
    /// own player alike. Each is one press and one release, the same shape a
    /// keyboard sends.
    fn send_media(&self, command: MediaCommand) -> Result<(), String> {
        deliver_media(media_code(command))
    }
}

/// Hands the keystrokes to the system.
#[cfg(windows)]
fn deliver(sequence: &[KeyStroke]) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VIRTUAL_KEY,
    };

    let inputs: Vec<INPUT> = sequence
        .iter()
        .map(|stroke| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(stroke.code),
                    wScan: 0,
                    dwFlags: if stroke.up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        })
        .collect();

    // SAFETY: the slice is a live array of `INPUT` records, and the size passed
    // with it is that of one record, which is what `SendInput` reads. Nothing
    // else in this crate builds an `INPUT`, so no union field other than `ki`
    // is ever written or read.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };

    // A short count means the system took some events and refused the rest -
    // typically because the foreground window belongs to a higher-integrity
    // process. Reporting it is the only way the window can say the keystroke
    // did not arrive; a half-sent combination is not a success. Whatever the
    // system did take may have been a modifier going down, and a modifier left
    // down is not a failed shortcut: it is a keyboard the user has to fix by
    // hand, with Ctrl or Alt stuck and the mouse behaving like it is held. So
    // every key this attempt could have pressed is released before the error
    // goes back, and the release is best-effort: it cannot fail worse than the
    // state it is repairing.
    if (sent as usize) < inputs.len() {
        release_attempted(&inputs);
        return Err(format!(
            "Windows accepted {sent} of {} key events, so the combination was not sent",
            inputs.len()
        ));
    }
    Ok(())
}

/// Releases every key `attempted` could have left down, ignoring the outcome.
///
/// The keys are released in the reverse order they were pressed in, which is the
/// order a hand would let go of them.
#[cfg(windows)]
fn release_attempted(attempted: &[windows::Win32::UI::Input::KeyboardAndMouse::INPUT]) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    let mut releases: Vec<INPUT> = attempted
        .iter()
        .map(|input| {
            let code = unsafe { input.Anonymous.ki.wVk };
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(code.0),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        })
        .collect();
    releases.reverse();

    // SAFETY: a live array of `INPUT` records with its own length, exactly as
    // `deliver` builds them. The result is deliberately ignored: this runs on
    // the path that is already reporting a failure.
    let _ = unsafe { SendInput(&releases, std::mem::size_of::<INPUT>() as i32) };
}

/// The virtual key a media command presses.
///
/// The four codes are Windows', not this project's: they are what a keyboard
/// sends for its own media keys, so a player cannot tell the difference.
fn media_code(command: MediaCommand) -> u16 {
    match command {
        MediaCommand::PlayPause => 0xB3,
        MediaCommand::Next => 0xB0,
        MediaCommand::Previous => 0xB1,
        MediaCommand::Stop => 0xB2,
    }
}

/// Presses and releases one media key.
#[cfg(windows)]
fn deliver_media(code: u16) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    let events = [false, true].map(|release| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(code),
                wScan: 0,
                dwFlags: match release {
                    true => KEYEVENTF_KEYUP,
                    false => Default::default(),
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });

    // SAFETY: `events` is a live array of `INPUT` records and its length is
    // passed with it; the call copies what it needs before returning.
    let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if (sent as usize) < events.len() {
        // The press may have landed even when the release did not, and a media
        // key left down is a player that skips tracks until the key repeats.
        // Releasing is best-effort: it cannot fail worse than the state it is
        // repairing.
        let release = [events[1]];
        // SAFETY: as above, for the one record that matters.
        let _ = unsafe { SendInput(&release, std::mem::size_of::<INPUT>() as i32) };
        return Err(format!(
            "Windows accepted {sent} of {} media key events",
            events.len()
        ));
    }
    Ok(())
}

/// Everywhere else there is no media key to send.
#[cfg(not(windows))]
fn deliver_media(_code: u16) -> Result<(), String> {
    Err("this platform has no way to send a media command yet".to_string())
}

/// Everywhere else there is no keystroke to send.
#[cfg(not(windows))]
fn deliver(_sequence: &[KeyStroke]) -> Result<(), String> {
    Err("this platform has no way to send keystrokes yet".to_string())
}

#[cfg(test)]
mod tests {
    use super::{base_code, modifier_code, named_code, punctuation_code, strokes, KeyStroke};
    use ss550_engine::{BaseKey, KeyCombo, Modifier, NamedKey};

    fn combo(text: &str) -> KeyCombo {
        KeyCombo::parse(text).expect("the test's own combination must parse")
    }

    fn press(code: u16) -> KeyStroke {
        KeyStroke { code, up: false }
    }

    fn release(code: u16) -> KeyStroke {
        KeyStroke { code, up: true }
    }

    #[test]
    fn every_modifier_has_its_own_documented_code() {
        let codes = [
            (Modifier::Ctrl, 0x11),
            (Modifier::Alt, 0x12),
            (Modifier::Shift, 0x10),
            (Modifier::Win, 0x5B),
        ];
        for (modifier, expected) in codes {
            assert_eq!(modifier_code(modifier), expected, "{modifier:?}");
        }
        // Two modifiers that shared a code would press the same key twice.
        let distinct: std::collections::BTreeSet<u16> = codes
            .iter()
            .map(|(modifier, _)| modifier_code(*modifier))
            .collect();
        assert_eq!(distinct.len(), codes.len());
    }

    #[test]
    fn letters_and_digits_land_on_the_virtual_key_range() {
        for letter in 'a'..='z' {
            let expected = u16::from(letter.to_ascii_uppercase() as u8);
            assert_eq!(
                base_code(BaseKey::Letter(letter)).expect("a letter has a code"),
                expected,
                "{letter}"
            );
        }
        for digit in '0'..='9' {
            assert_eq!(
                base_code(BaseKey::Digit(digit)).expect("a digit has a code"),
                u16::from(digit as u8),
                "{digit}"
            );
        }
        // `VK_A` is 0x41 and `VK_0` is 0x30: the ranges the platform documents.
        assert_eq!(base_code(BaseKey::Letter('a')).expect("code"), 0x41);
        assert_eq!(base_code(BaseKey::Digit('0')).expect("code"), 0x30);
    }

    #[test]
    fn the_function_keys_start_at_vk_f1_and_end_at_vk_f24() {
        assert_eq!(base_code(BaseKey::Function(1)).expect("code"), 0x70);
        assert_eq!(base_code(BaseKey::Function(24)).expect("code"), 0x87);

        // Every key the vocabulary allows has its own code, with no gaps.
        let codes: std::collections::BTreeSet<u16> = (1..=24)
            .map(|number| base_code(BaseKey::Function(number)).expect("code"))
            .collect();
        assert_eq!(codes.len(), 24, "two function keys share a code");
    }

    #[test]
    fn every_named_key_has_the_code_windows_documents() {
        let codes = [
            (NamedKey::Up, 0x26),
            (NamedKey::Down, 0x28),
            (NamedKey::Left, 0x25),
            (NamedKey::Right, 0x27),
            (NamedKey::Enter, 0x0D),
            (NamedKey::Escape, 0x1B),
            (NamedKey::Tab, 0x09),
            (NamedKey::Space, 0x20),
            (NamedKey::Backspace, 0x08),
            (NamedKey::Delete, 0x2E),
            (NamedKey::Home, 0x24),
            (NamedKey::End, 0x23),
            (NamedKey::PageUp, 0x21),
            (NamedKey::PageDown, 0x22),
            (NamedKey::Insert, 0x2D),
        ];
        for (key, expected) in codes {
            assert_eq!(named_code(key), expected, "{key:?}");
        }
        let distinct: std::collections::BTreeSet<u16> =
            codes.iter().map(|(key, _)| named_code(*key)).collect();
        assert_eq!(distinct.len(), codes.len(), "two named keys share a code");
    }

    #[test]
    fn every_punctuation_key_the_engine_accepts_has_an_oem_code() {
        // The engine's documented set, and the code each one sits on.
        let marks = [
            (',', 0xBC),
            ('.', 0xBE),
            ('/', 0xBF),
            (';', 0xBA),
            ('\'', 0xDE),
            ('[', 0xDB),
            (']', 0xDD),
            ('\\', 0xDC),
            ('-', 0xBD),
            ('=', 0xBB),
            ('`', 0xC0),
        ];
        for (mark, expected) in marks {
            assert_eq!(
                punctuation_code(mark).expect("a code"),
                expected,
                "{mark:?}"
            );
            assert_eq!(
                base_code(BaseKey::Punctuation(mark)).expect("a code"),
                expected,
                "{mark:?}"
            );
        }
        let distinct: std::collections::BTreeSet<u16> = marks
            .iter()
            .map(|(mark, _)| punctuation_code(*mark).expect("a code"))
            .collect();
        assert_eq!(distinct.len(), marks.len());
    }

    #[test]
    fn a_mark_outside_the_vocabulary_is_refused_instead_of_guessed() {
        // `BaseKey` is public, so a mark the parser would never build can still
        // be handed in. It must be a sentence, not a wrong key press.
        assert!(punctuation_code('@').is_err());
        assert!(base_code(BaseKey::Punctuation('@')).is_err());
    }

    #[test]
    fn a_combination_presses_the_modifiers_then_the_key_and_releases_in_reverse() {
        assert_eq!(
            strokes(&combo("ctrl+shift+m")).expect("a sequence"),
            vec![
                press(0x11),   // Ctrl down
                press(0x10),   // Shift down
                press(0x4D),   // M down
                release(0x4D), // M up
                release(0x10), // Shift up
                release(0x11), // Ctrl up
            ]
        );
    }

    #[test]
    fn the_spelling_of_a_combination_never_changes_what_is_sent() {
        // The engine sorts its modifiers; this crate states its own order. The
        // two must agree whatever order the user typed.
        let canonical = strokes(&combo("ctrl+alt+shift+win+m")).expect("a sequence");
        for spelling in [
            "m+win+shift+alt+ctrl",
            "SHIFT + CTRL + M + ALT + WIN",
            "win+ctrl+alt+shift+m",
        ] {
            assert_eq!(
                strokes(&combo(spelling)).expect("a sequence"),
                canonical,
                "{spelling:?}"
            );
        }
        // And the order is the documented one, not the alphabetical one.
        assert_eq!(
            canonical[..4],
            [press(0x11), press(0x12), press(0x10), press(0x5B)],
            "modifiers go down ctrl, alt, shift, win"
        );
    }

    #[test]
    fn a_key_on_its_own_is_one_press_and_one_release() {
        assert_eq!(
            strokes(&combo("f24")).expect("a sequence"),
            vec![press(0x87), release(0x87)]
        );
        assert_eq!(
            strokes(&combo("enter")).expect("a sequence"),
            vec![press(0x0D), release(0x0D)]
        );
    }

    #[test]
    fn every_combination_that_parses_sends_balanced_keystrokes() {
        // The invariant that keeps a modifier from being stuck down: every
        // release belongs to a press that already happened, and nothing is left
        // held at the end.
        let combos = [
            "m",
            "ctrl+m",
            "ctrl+alt+shift+win+backspace",
            "win+pagedown",
            "alt+tab",
            "shift+=",
            "ctrl+\\",
            "f1",
            "ctrl+f12",
            "super+space",
        ];
        for text in combos {
            let sequence = strokes(&combo(text)).expect("a sequence");
            let mut held: Vec<u16> = Vec::new();
            for stroke in &sequence {
                if stroke.up {
                    let position = held
                        .iter()
                        .position(|code| *code == stroke.code)
                        .unwrap_or_else(|| panic!("{text:?} releases a key that was not down"));
                    held.remove(position);
                } else {
                    assert!(
                        !held.contains(&stroke.code),
                        "{text:?} presses a key that is already down"
                    );
                    held.push(stroke.code);
                }
            }
            assert!(held.is_empty(), "{text:?} leaves a key held down");
            // The key itself is pressed exactly once.
            assert_eq!(
                sequence.iter().filter(|stroke| !stroke.up).count(),
                combo(text).modifiers().len() + 1,
                "{text:?}"
            );
        }
    }
}

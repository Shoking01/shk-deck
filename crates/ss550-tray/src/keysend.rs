//! Sending key combinations with Win32 `SendInput`.
//!
//! A combination is delivered the way a hand would type it: the modifiers go
//! down in a fixed order, the key goes down, it is held for a moment, and then
//! it comes back up with the modifiers released in the reverse order. The order
//! is fixed on purpose. `Ctrl`, `Alt`, `Shift`, `Win` is the order the system
//! itself reports, and releasing in the reverse of the presses is what leaves no
//! modifier stuck down when a send fails halfway.
//!
//! ## Scan codes, not virtual keys
//!
//! Every event leaves here carrying a **hardware scan code** (`wScan` with
//! `KEYEVENTF_SCANCODE`), never a bare virtual key. A scan code is what the
//! keyboard sends and what Windows turns into a virtual key; a virtual key on
//! its own is the derived half. Text fields read the derived key, so a bare
//! virtual key types in Notepad — but everything that reads the keyboard *as a
//! device* reads the scan code, and Raw Input reports `MakeCode = 0` for a
//! virtual-key-only event. That is a key no keyboard has, and a game binding a
//! key never sees it fire. Keys a keyboard prefixes with `E0` (the arrows, the
//! navigation cluster, the media keys) keep that prefix as
//! `KEYEVENTF_EXTENDEDKEY`; without it the same scan code arrives as the
//! keypad's twin.
//!
//! Between the presses and the releases the keys are held for
//! [`HOLD`]: a real key is held for tens of milliseconds, and pressing and
//! releasing inside one input tick is invisible to a game that samples the
//! keyboard once per frame.
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
//!
//! The virtual key is an intermediate step: [`system_scan_code`] turns it into
//! the scan code the layout pairs with it, and that is what goes out.

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

/// One key event as the system's input stack takes it: a **hardware scan code**.
///
/// A scan code is what a keyboard sends and a virtual key is what Windows
/// derives from it, so only the scan code survives the trip to a game. See the
/// module documentation for why this crate never sends a bare virtual key.
///
/// Windows is the only platform with a table to translate into, so off Windows
/// this type exists for its own tests and nothing else.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ScanStroke {
    /// The scan code, without its `E0` prefix.
    scan: u16,
    /// True when a keyboard prefixes this key with `E0`.
    extended: bool,
    /// True when the key is released.
    up: bool,
}

/// The virtual keys a keyboard prefixes with `E0` that `MapVirtualKey` does not
/// report that way.
///
/// Windows answers the navigation cluster with a **plain** scan code even though
/// a keyboard sends `E0 48` for Up; sending the plain code would arrive as the
/// keypad's `8`. These are the cluster's keys in the engine's vocabulary, taken
/// from the set Windows itself documents as extended.
///
/// Windows is the only platform whose keyboard layout is asked, so off Windows
/// this exists for the test that drives [`scan_strokes`] and nothing else.
#[cfg(any(windows, test))]
fn is_extended(code: u16) -> bool {
    matches!(
        code,
        0x21..=0x28 // PageUp, PageDown, End, Home, Left, Up, Right, Down
        | 0x2D      // Insert
        | 0x2E // Delete
    )
}

/// Translates the virtual keys of a sequence into the scan codes a keyboard sends.
///
/// `lookup` is the platform's virtual-key-to-scan-code table, passed in so the
/// translation is a plain function a test can drive. A key the table does not
/// answer for is refused with a sentence rather than sent as scan code zero:
/// zero is not a key any keyboard has.
///
/// Windows is the only platform with a table to translate into, so off Windows
/// this exists for its own test and nothing else.
#[cfg(any(windows, test))]
fn scan_strokes(
    sequence: &[KeyStroke],
    lookup: impl Fn(u16) -> Option<u16>,
) -> Result<Vec<ScanStroke>, String> {
    sequence
        .iter()
        .map(|stroke| {
            let packed = lookup(stroke.code).ok_or_else(|| {
                format!(
                    "the key 0x{:02X} has no hardware scan code on this keyboard layout",
                    stroke.code
                )
            })?;
            Ok(ScanStroke {
                scan: packed & 0xFF,
                // `MapVirtualKey` puts the E0 prefix in the high byte, but only
                // for the keys it counts as extended; the navigation cluster is
                // the documented exception.
                extended: (packed >> 8) & 0xFF == 0xE0 || is_extended(stroke.code),
                up: stroke.up,
            })
        })
        .collect()
}

/// The scan code the keyboard layout pairs with a virtual key, E0 prefix and all.
#[cfg(windows)]
fn system_scan_code(code: u16) -> Option<u16> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_VSC_EX};

    // SAFETY: `MapVirtualKeyW` only reads the keyboard layout and answers a
    // value; it touches no memory this process owns. Zero means the layout has
    // no scan code for this key, which is a refusal, not a key press.
    let packed = unsafe { MapVirtualKeyW(u32::from(code), MAPVK_VK_TO_VSC_EX) };
    u16::try_from(packed).ok().filter(|packed| *packed != 0)
}

/// How long the keys are held down between the presses and the releases.
///
/// A hand holds a key for tens of milliseconds. Pressing and releasing inside
/// one tick is invisible to a game that samples the keyboard once per frame, so
/// the presses go out, the keys stay down a little over one frame at 60 Hz, and
/// then they come back up. It is short enough to stay inside the deck's latency
/// budget and long enough for a frame to see it.
#[cfg(windows)]
const HOLD: std::time::Duration = std::time::Duration::from_millis(20);

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
///
/// The presses go out first, then the keys are held for [`HOLD`], then the
/// releases go out. Every record carries a scan code; the module documentation
/// says why.
#[cfg(windows)]
fn deliver(sequence: &[KeyStroke]) -> Result<(), String> {
    let strokes = scan_strokes(sequence, system_scan_code)?;

    // `strokes` is presses followed by releases, so the first release is the
    // boundary between the two batches.
    let split = strokes
        .iter()
        .position(|stroke| stroke.up)
        .unwrap_or(strokes.len());
    let (presses, releases) = strokes.split_at(split);

    if let Err(counts) = send_batch(presses) {
        // Whatever the system did take may have been a modifier going down, and
        // a modifier left down is not a failed shortcut: it is a keyboard the
        // user has to fix by hand, with Ctrl or Alt stuck and the mouse behaving
        // like it is held. So every key this attempt could have pressed is
        // released before the error goes back, and the release is best-effort:
        // it cannot fail worse than the state it is repairing.
        release_attempted(presses);
        return Err(format!(
            "Windows accepted {counts} key presses, so the combination was not sent"
        ));
    }

    std::thread::sleep(HOLD);

    if let Err(counts) = send_batch(releases) {
        // The presses landed, so a key may still be down. Releasing again is
        // best-effort, and the sentence says that is what happened.
        release_attempted(presses);
        return Err(format!(
            "Windows accepted {counts} key releases; every key was released again"
        ));
    }

    Ok(())
}

/// Builds the `INPUT` record one scan-code stroke is sent as.
#[cfg(windows)]
fn input_for(stroke: &ScanStroke) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
        KEYEVENTF_SCANCODE, VIRTUAL_KEY,
    };

    let mut flags = KEYEVENTF_SCANCODE;
    if stroke.extended {
        // Without this prefix the same scan code arrives as the keypad's twin.
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if stroke.up {
        flags |= KEYEVENTF_KEYUP;
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                // A scan-code event ignores the virtual key; stating zero keeps
                // it obvious that none was derived here.
                wVk: VIRTUAL_KEY(0),
                wScan: stroke.scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Hands one batch of scan-code strokes to the system.
///
/// The error is the short count as `sent of expected`: `SendInput` refuses the
/// rest when, for instance, the foreground window belongs to a higher-integrity
/// process, and only the caller knows what the loss means for the combination.
#[cfg(windows)]
fn send_batch(strokes: &[ScanStroke]) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};

    if strokes.is_empty() {
        return Ok(());
    }

    let inputs: Vec<INPUT> = strokes.iter().map(input_for).collect();

    // SAFETY: the slice is a live array of `INPUT` records, and the size passed
    // with it is that of one record, which is what `SendInput` reads. Nothing
    // else in this crate builds an `INPUT`, so no union field other than `ki`
    // is ever written or read.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };

    if (sent as usize) < inputs.len() {
        return Err(format!("{sent} of {}", inputs.len()));
    }
    Ok(())
}

/// Releases every key `pressed` could have left down, ignoring the outcome.
///
/// The keys are released in the reverse order they were pressed in, which is the
/// order a hand would let go of them, and each release carries the same scan
/// code and `E0` prefix as the press it is undoing.
#[cfg(windows)]
fn release_attempted(pressed: &[ScanStroke]) {
    let mut releases: Vec<ScanStroke> = pressed
        .iter()
        .map(|stroke| ScanStroke {
            up: true,
            ..*stroke
        })
        .collect();
    releases.reverse();

    // The result is deliberately ignored: this runs on the path that is already
    // reporting a failure.
    let _ = send_batch(&releases);
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
///
/// It goes out through the same path as a combination, so it carries the `E0`
/// prefix the key has on a real keyboard; without it the same scan code is a
/// different key to anything reading the device.
#[cfg(windows)]
fn deliver_media(code: u16) -> Result<(), String> {
    deliver(&[KeyStroke { code, up: false }, KeyStroke { code, up: true }])
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
    #[cfg(windows)]
    use super::media_code;
    use super::{
        base_code, modifier_code, named_code, punctuation_code, scan_strokes, strokes, KeyStroke,
        ScanStroke,
    };
    #[cfg(windows)]
    use ss550_engine::MediaCommand;
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

    /// A scan-code stroke, for the tests that do not care about the lookup.
    fn scanned(scan: u16, up: bool) -> ScanStroke {
        ScanStroke {
            scan,
            extended: false,
            up,
        }
    }

    #[test]
    fn every_press_becomes_a_scan_code_stroke_and_nothing_else_changes() {
        // The translation keeps the order and the press/release sides exactly as
        // `strokes` built them: it only swaps a virtual key for a scan code.
        let sequence = strokes(&combo("ctrl+shift+m")).expect("a sequence");
        let translated = scan_strokes(&sequence, Some).expect("a mapping");

        assert_eq!(translated.len(), sequence.len());
        for (before, after) in sequence.iter().zip(&translated) {
            assert_eq!(after.scan, before.code, "{before:?}");
            assert_eq!(after.up, before.up, "{before:?}");
        }
        assert_eq!(
            translated,
            vec![
                scanned(0x11, false), // Ctrl
                scanned(0x10, false), // Shift
                scanned(0x4D, false), // M
                scanned(0x4D, true),
                scanned(0x10, true),
                scanned(0x11, true),
            ]
        );
    }

    #[test]
    fn the_e0_prefix_is_what_marks_a_key_extended() {
        // `MapVirtualKey` answers `0xE0` in the high byte for a key a keyboard
        // prefixes with E0. Losing that byte would send "numpad 8" where the
        // user asked for the Up arrow.
        let sequence = strokes(&combo("up")).expect("a sequence");
        let translated = scan_strokes(&sequence, |_| Some(0xE0_48)).expect("a mapping");
        assert_eq!(
            translated,
            vec![
                ScanStroke {
                    scan: 0x48,
                    extended: true,
                    up: false,
                },
                ScanStroke {
                    scan: 0x48,
                    extended: true,
                    up: true,
                },
            ]
        );
    }

    #[test]
    fn the_navigation_cluster_is_extended_even_when_the_table_answers_a_plain_code() {
        // Windows answers `VK_UP` with `0x48`, the same number the keypad's `8`
        // has; only the E0 prefix tells them apart.
        let sequence = strokes(&combo("up")).expect("a sequence");
        let translated = scan_strokes(&sequence, |_| Some(0x48)).expect("a mapping");
        assert!(translated
            .iter()
            .all(|stroke| stroke.extended && stroke.scan == 0x48));
    }

    #[test]
    fn a_key_without_a_scan_code_is_refused_instead_of_sent_as_zero() {
        // The bug this replaced: a virtual key with no scan code went out as
        // `wScan: 0`, and every window that reads the keyboard as a keyboard
        // (a game, Raw Input, DirectInput) saw a key that does not exist while
        // text fields still accepted the virtual key.
        let sequence = strokes(&combo("f24")).expect("a sequence");
        let error = scan_strokes(&sequence, |_| None).expect_err("no mapping, no send");
        assert!(error.contains("scan code"), "{error}");
        // `VK_F24` is `0x87`; the sentence names the key so the log can be read.
        assert!(error.contains("0x87"), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn the_media_keys_keep_the_e0_prefix_a_keyboard_sends() {
        // A media key without its prefix is a different key to a player that
        // reads the device, so the media path is pinned here as well.
        for command in [
            MediaCommand::PlayPause,
            MediaCommand::Next,
            MediaCommand::Previous,
            MediaCommand::Stop,
        ] {
            let sequence = [KeyStroke {
                code: media_code(command),
                up: false,
            }];
            let translated = scan_strokes(&sequence, super::system_scan_code)
                .unwrap_or_else(|error| panic!("{command:?}: {error}"));
            assert!(
                translated[0].extended,
                "{command:?} must keep its E0 prefix"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn every_key_the_engine_can_build_has_a_hardware_scan_code() {
        // The promise of the fix, taken from the system itself: no key in the
        // vocabulary may translate to zero.
        let mut keys: Vec<BaseKey> = Vec::new();
        for letter in 'a'..='z' {
            keys.push(BaseKey::Letter(letter));
        }
        for digit in '0'..='9' {
            keys.push(BaseKey::Digit(digit));
        }
        for number in 1..=24 {
            keys.push(BaseKey::Function(number));
        }
        for text in [
            "up",
            "down",
            "left",
            "right",
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
        ] {
            match KeyCombo::parse(text)
                .expect("the test's own name must parse")
                .base_key()
            {
                BaseKey::Named(named) => keys.push(BaseKey::Named(named)),
                other => panic!("{other:?} is not a named key"),
            }
        }
        for mark in [',', '.', '/', ';', '\'', '[', ']', '\\', '-', '=', '`'] {
            keys.push(BaseKey::Punctuation(mark));
        }

        for key in keys {
            let code = base_code(key).expect("the vocabulary has a virtual key");
            let packed = super::system_scan_code(code)
                .unwrap_or_else(|| panic!("{key:?} has no hardware scan code"));
            assert_ne!(
                packed & 0xFF,
                0,
                "{key:?} maps to scan code 0, which is a key no keyboard has"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn the_keys_a_keyboard_prefixes_with_e0_are_marked_extended() {
        for text in [
            "up", "down", "left", "right", "home", "end", "pageup", "pagedown", "insert", "delete",
        ] {
            let sequence = strokes(&combo(text)).expect("a sequence");
            let translated = scan_strokes(&sequence, super::system_scan_code)
                .unwrap_or_else(|error| panic!("{text:?}: {error}"));
            assert!(
                translated.iter().all(|stroke| stroke.extended),
                "{text:?} must keep its E0 prefix"
            );
        }

        // A key that a keyboard sends without a prefix must not grow one: E0
        // would turn it into a different key on the receiving side.
        for text in ["a", "1", "f1", "enter", "tab", "space", "backspace"] {
            let sequence = strokes(&combo(text)).expect("a sequence");
            let translated = scan_strokes(&sequence, super::system_scan_code)
                .unwrap_or_else(|error| panic!("{text:?}: {error}"));
            assert!(
                translated.iter().all(|stroke| !stroke.extended),
                "{text:?} must not be prefixed"
            );
        }
    }
}

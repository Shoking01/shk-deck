//! A round trip through the real logon key.
//!
//! This test writes to the user's own `Run` key, so it is ignored by default
//! and only runs when it is asked for by name:
//!
//! ```text
//! cargo test -p ss550-tray -- --ignored autostart_round_trip
//! ```
//!
//! It puts back whatever was there before, so a machine that already starts
//! the controller at logon keeps doing so.

#![cfg(windows)]

use ss550_tray::autostart;

/// The value the test writes, so it can tell its own entry from a real one.
const TEST_COMMAND: &str = r#""C:\ss550 test\app.exe" --hidden"#;

#[test]
#[ignore = "writes to the user's HKCU Run key"]
fn autostart_round_trip() {
    let original = read_entry();

    autostart::enable(TEST_COMMAND).expect("the entry could be written");
    assert!(autostart::is_enabled().expect("the entry can be read"));
    assert_eq!(
        read_entry().as_deref(),
        Some(TEST_COMMAND),
        "the entry holds the command line that was written"
    );

    autostart::disable().expect("the entry could be removed");
    assert!(!autostart::is_enabled().expect("the entry can be read"));
    assert_eq!(read_entry(), None, "the entry is gone");

    // Removing it twice is not an error.
    autostart::disable().expect("removing an absent entry is fine");

    if let Some(original) = original {
        autostart::enable(&original).expect("the previous entry could be put back");
    }
}

/// Reads the raw string, so the test can restore a value the crate would
/// otherwise only report as a bool.
fn read_entry() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let run = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .ok()?;
    run.get_value::<String, _>(autostart::VALUE_NAME).ok()
}

//! A round trip through the real logon key and the real install folder.
//!
//! These tests write to the user's own `Run` key and copy a file into
//! `%LOCALAPPDATA%\Programs\ss550`, so they are ignored by default and only run
//! when asked for by name:
//!
//! ```text
//! cargo test -p ss550-tray -- --ignored
//! ```
//!
//! Everything they touch is put back, so a machine that already starts the
//! controller at logon, or already has a controller installed, keeps it.

#![cfg(windows)]

use ss550_tray::{autostart, install};

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

/// The whole story the GUI performs when the switch is turned on: install,
/// register the installed path, and report a truthful state at startup.
#[test]
#[ignore = "writes to the user's HKCU Run key and to LOCALAPPDATA"]
fn installed_autostart_round_trip() {
    let original_entry = read_entry();
    let installed = install::installed_executable().expect("LOCALAPPDATA is set");
    let original_file = std::fs::read(&installed).ok();

    let scratch = std::env::temp_dir().join(format!("ss550-running-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("the scratch folder can be created");
    let running = scratch.join("ss550-gui.exe");
    std::fs::write(&running, b"a build that lives in a target folder").expect("can be written");

    let result = std::panic::catch_unwind(|| {
        let installed_path =
            install::ensure_installed(&running).expect("the controller can be installed");
        assert_eq!(installed_path, installed);
        assert_eq!(
            std::fs::read(&installed_path).expect("the installed copy is readable"),
            std::fs::read(&running).expect("the source is readable"),
            "the installed copy is the running executable"
        );

        autostart::enable(&autostart::command_line(&installed_path, true))
            .expect("the entry can be written");
        assert_eq!(
            autostart::sign_in().expect("the entry can be read"),
            autostart::SignIn::On(installed_path.clone()),
            "a registration that points at a real file reads as on"
        );

        // A registration that survives while its file does not must not read
        // as on: that is the switch that lies.
        let gone = scratch.join("gone.exe");
        autostart::enable(&autostart::command_line(&gone, true))
            .expect("a stale entry can be written");
        match autostart::sign_in().expect("the entry can be read") {
            autostart::SignIn::Broken { executable, .. } => {
                assert_eq!(executable.as_deref(), Some(gone.as_path()));
            }
            other => panic!("a missing executable must read as broken, got {other:?}"),
        }

        autostart::disable().expect("the entry can be removed");
        assert_eq!(
            autostart::sign_in().expect("the entry can be read"),
            autostart::SignIn::Off
        );
    });

    // Put the machine back the way it was, whatever happened above.
    match original_file {
        Some(bytes) => {
            std::fs::write(&installed, bytes).expect("the previous install can be restored")
        }
        None => {
            std::fs::remove_file(&installed).ok();
            if let Some(folder) = installed.parent() {
                std::fs::remove_dir(folder).ok();
            }
        }
    }
    match original_entry {
        Some(command) => autostart::enable(&command).expect("the previous entry can be restored"),
        None => autostart::disable().expect("the test entry can be removed"),
    }
    std::fs::remove_dir_all(&scratch).ok();

    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

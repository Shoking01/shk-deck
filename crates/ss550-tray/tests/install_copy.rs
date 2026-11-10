//! The copy into the per-user folder, without touching the real profile.
//!
//! `LOCALAPPDATA` is pointed at a scratch folder for the duration of the test,
//! so nothing is written outside `%TEMP%`. The registry half of the feature
//! lives in `autostart_round_trip.rs`, which needs the real key and is ignored.
//!
//! The three checks run in one test on purpose: they share the process-wide
//! `LOCALAPPDATA`, and parallel tests in one binary would race on it.

#![cfg(windows)]

use std::path::PathBuf;

use ss550_tray::autostart::command_line;
use ss550_tray::install;

#[test]
fn a_build_is_copied_into_the_per_user_folder_and_reinstalling_is_safe() {
    let root = std::env::temp_dir().join(format!("ss550-install-test-{}", std::process::id()));
    let local = root.join("local");
    std::fs::create_dir_all(&local).expect("the scratch folder can be created");

    let previous = std::env::var_os("LOCALAPPDATA");
    std::env::set_var("LOCALAPPDATA", &local);

    let outcome = std::panic::catch_unwind(|| {
        let expected: PathBuf = local.join("Programs").join("ss550").join("ss550-gui.exe");
        assert_eq!(
            install::installed_executable().expect("the install folder is known"),
            expected
        );
        assert_eq!(
            command_line(&expected, true),
            format!("\"{}\" --hidden", expected.display()),
            "the logon entry is quoted and hidden"
        );

        // A build that lives somewhere else is copied, not referenced.
        let build = root.join("build").join("ss550-gui.exe");
        std::fs::create_dir_all(build.parent().expect("the build folder is named"))
            .expect("the build folder can be created");
        std::fs::write(&build, b"first build").expect("the build can be written");

        let installed = install::ensure_installed(&build).expect("the first install works");
        assert_eq!(installed, expected);
        assert_eq!(
            std::fs::read(&installed).expect("the copy is readable"),
            b"first build"
        );

        // Installing again must not fail or change anything.
        install::ensure_installed(&build).expect("the second install works");
        assert_eq!(
            std::fs::read(&installed).expect("the copy is readable"),
            b"first build"
        );

        // A newer build replaces the copy.
        std::fs::write(&build, b"second build").expect("the build can be rewritten");
        install::ensure_installed(&build).expect("the reinstall works");
        assert_eq!(
            std::fs::read(&installed).expect("the copy is readable"),
            b"second build"
        );

        // Running from the install folder must be a no-op, not a copy onto
        // the file that is in use.
        let in_place = install::ensure_installed(&installed).expect("installing in place works");
        assert_eq!(in_place, installed);
        assert_eq!(
            std::fs::read(&installed).expect("the copy is readable"),
            b"second build"
        );
    });

    match previous {
        Some(value) => std::env::set_var("LOCALAPPDATA", value),
        None => std::env::remove_var("LOCALAPPDATA"),
    }
    std::fs::remove_dir_all(&root).ok();

    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

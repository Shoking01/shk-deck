//! "Start with Windows", without an installer.
//!
//! The registration is a single value under
//! `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`, so it
//! only affects the user who turned it on, needs no administrator rights,
//! leaves no service behind and disappears completely when it is turned off
//! again.

use std::path::Path;

use crate::TrayError;

/// The name of the registry value the controller writes.
pub const VALUE_NAME: &str = "ss550-gui";

/// The flag that tells the controller to start without a window.
pub const HIDDEN_FLAG: &str = "--hidden";

/// Builds the command line the logon entry runs.
///
/// The path is quoted, because a program folder may contain spaces.
///
/// # Example
///
/// ```
/// use std::path::Path;
///
/// let command = ss550_tray::autostart::command_line(
///     Path::new(r"C:\Program Files\SS550\controller.exe"),
///     true,
/// );
/// assert_eq!(
///     command,
///     r#""C:\Program Files\SS550\controller.exe" --hidden"#
/// );
/// ```
pub fn command_line(executable: &Path, hidden: bool) -> String {
    let mut command = format!("\"{}\"", executable.display());
    if hidden {
        command.push(' ');
        command.push_str(HIDDEN_FLAG);
    }
    command
}

/// Whether the controller is registered to start at logon.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// if ss550_tray::autostart::is_enabled()? {
///     println!("the controller starts with Windows");
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn is_enabled() -> Result<bool, TrayError> {
    let run = open_run_key()?;
    match run.get_value::<String, _>(VALUE_NAME) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(TrayError::Registry(error.to_string())),
    }
}

/// Registers the controller to start at logon.
///
/// The command line is written as it is, so it has to be built with
/// [`command_line`].
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// let command = ss550_tray::autostart::command_line(
///     std::path::Path::new(r"C:\apps\controller.exe"),
///     true,
/// );
/// ss550_tray::autostart::enable(&command)?;
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn enable(command: &str) -> Result<(), TrayError> {
    let run = open_run_key()?;
    run.set_value(VALUE_NAME, &command.to_string())
        .map_err(|error| TrayError::Registry(error.to_string()))
}

/// Removes the logon registration.
///
/// Removing a registration that is not there is not an error: the wanted
/// state is already the one that is true.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// ss550_tray::autostart::disable()?;
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn disable() -> Result<(), TrayError> {
    let run = open_run_key()?;
    match run.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(TrayError::Registry(error.to_string())),
    }
}

/// The registry key Windows runs at logon.
#[cfg(windows)]
fn open_run_key() -> Result<winreg::RegKey, TrayError> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, KEY_WRITE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_PARENT: &str = r"Software\Microsoft\Windows\CurrentVersion";

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = current_user.open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_SET_VALUE) {
        return Ok(key);
    }
    // The Run key belongs to every Windows profile, but a trimmed one may
    // not have it yet.
    let parent = current_user
        .open_subkey_with_flags(RUN_PARENT, KEY_READ | KEY_WRITE)
        .map_err(|error| TrayError::Registry(format!("{RUN_PARENT}: {error}")))?;
    parent
        .create_subkey("Run")
        .map(|(key, _)| key)
        .map_err(|error| TrayError::Registry(format!("{RUN_KEY}: {error}")))
}

/// The logon entry is a Windows thing; elsewhere there is none.
#[cfg(not(windows))]
pub fn is_enabled() -> Result<bool, TrayError> {
    Err(TrayError::Unsupported)
}

/// The logon entry is a Windows thing; elsewhere there is none.
#[cfg(not(windows))]
pub fn enable(_command: &str) -> Result<(), TrayError> {
    Err(TrayError::Unsupported)
}

/// The logon entry is a Windows thing; elsewhere there is none.
#[cfg(not(windows))]
pub fn disable() -> Result<(), TrayError> {
    Err(TrayError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_quotes_the_path() {
        let command = command_line(Path::new(r"C:\Program Files\SS550\app.exe"), false);
        assert_eq!(command, r#""C:\Program Files\SS550\app.exe""#);
    }

    #[test]
    fn the_hidden_flag_is_only_added_when_it_is_asked_for() {
        let hidden = command_line(Path::new(r"C:\app\app.exe"), true);
        let visible = command_line(Path::new(r"C:\app\app.exe"), false);
        assert_eq!(hidden, r#""C:\app\app.exe" --hidden"#);
        assert!(!visible.contains("--hidden"));
    }

    #[test]
    fn the_hidden_flag_is_the_one_the_app_looks_for() {
        assert_eq!(HIDDEN_FLAG, "--hidden");
    }
}

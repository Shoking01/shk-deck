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

/// What Windows would start at sign-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    /// No logon entry.
    Off,
    /// An entry that points at a file which is there.
    On(std::path::PathBuf),
    /// An entry that still exists but names a file that does not.
    Broken {
        /// The value as it is stored.
        command: String,
        /// The executable it names, when even that could be read.
        executable: Option<std::path::PathBuf>,
    },
}

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
/// This answers only "is the value there?". Use [`sign_in`] when the question
/// is "will Windows actually start something?".
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

/// The command line the logon entry holds, exactly as it was written.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// if let Some(command) = ss550_tray::autostart::registered_command()? {
///     println!("Windows runs: {command}");
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn registered_command() -> Result<Option<String>, TrayError> {
    let run = open_run_key()?;
    match run.get_value::<String, _>(VALUE_NAME) {
        Ok(command) => Ok(Some(command)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(TrayError::Registry(error.to_string())),
    }
}

/// What Windows would actually start when the user signs in.
///
/// A registration is not the same as a working one: the value can still name a
/// file that was deleted or moved, and then the logon attempt fails silently
/// while the app keeps reporting that the feature is on.
///
/// # Example
///
/// ```no_run
/// # fn main() -> Result<(), ss550_tray::TrayError> {
/// use ss550_tray::autostart::SignIn;
///
/// match ss550_tray::autostart::sign_in()? {
///     SignIn::Off => println!("the controller is not started at sign-in"),
///     SignIn::On(path) => println!("Windows starts {}", path.display()),
///     SignIn::Broken { command, .. } => {
///         println!("Windows would try to start something that is gone: {command}")
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(windows)]
pub fn sign_in() -> Result<SignIn, TrayError> {
    let Some(command) = registered_command()? else {
        return Ok(SignIn::Off);
    };
    let executable = command_line_executable(&command);
    match executable {
        Some(path) if path.is_file() => Ok(SignIn::On(path)),
        executable => Ok(SignIn::Broken {
            command,
            executable,
        }),
    }
}

/// The executable a command line produced by [`command_line`] runs.
///
/// The path is quoted when it contains spaces, and Windows strips that pair
/// before it runs anything; an unquoted command is split at the first space.
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use ss550_tray::autostart::command_line_executable;
///
/// let command = r#""C:\Program Files\SS550\app.exe" --hidden"#;
/// assert_eq!(
///     command_line_executable(command).as_deref(),
///     Some(Path::new(r"C:\Program Files\SS550\app.exe"))
/// );
/// ```
pub fn command_line_executable(command: &str) -> Option<std::path::PathBuf> {
    let command = command.trim();
    let path = if let Some(rest) = command.strip_prefix('"') {
        &rest[..rest.find('"')?]
    } else {
        command.split_whitespace().next()?
    };
    if path.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(path))
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
pub fn registered_command() -> Result<Option<String>, TrayError> {
    Err(TrayError::Unsupported)
}

/// The logon entry is a Windows thing; elsewhere there is none.
#[cfg(not(windows))]
pub fn sign_in() -> Result<SignIn, TrayError> {
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

    #[test]
    fn a_quoted_command_line_names_its_executable() {
        let command = command_line(Path::new(r"C:\Program Files\SS550\app.exe"), true);
        assert_eq!(
            command_line_executable(&command).as_deref(),
            Some(Path::new(r"C:\Program Files\SS550\app.exe"))
        );
    }

    #[test]
    fn an_unquoted_command_line_is_split_at_the_first_space() {
        assert_eq!(
            command_line_executable(r"C:\app\app.exe --hidden").as_deref(),
            Some(Path::new(r"C:\app\app.exe"))
        );
    }

    #[test]
    fn a_command_line_without_a_program_names_nothing() {
        assert_eq!(command_line_executable(""), None);
        assert_eq!(command_line_executable("   "), None);
        assert_eq!(command_line_executable(r#""""#), None);
        assert_eq!(command_line_executable(r#""unterminated"#), None);
    }
}

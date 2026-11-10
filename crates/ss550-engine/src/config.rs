//! Where the app keeps its files.

use std::env;
use std::path::PathBuf;

/// The environment variable that overrides the configuration directory.
///
/// It exists so the tests (and anyone debugging a broken install) can point
/// the app at a folder of their own.
pub const CONFIG_DIR_ENV: &str = "SS550_CONFIG_DIR";

/// The folder the app owns inside the user's configuration.
const APP_DIR: &str = "ss550";

/// The file the bindings live in.
const BINDINGS_FILE: &str = "bindings.json";

/// The configuration directory for this app.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = env::var_os(CONFIG_DIR_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    default_config_dir()
}

/// Where the bindings file lives.
pub fn bindings_path() -> PathBuf {
    config_dir().join(BINDINGS_FILE)
}

/// The platform's own idea of where this app belongs.
#[cfg(windows)]
fn default_config_dir() -> PathBuf {
    match env::var_os("APPDATA") {
        Some(base) if !base.is_empty() => PathBuf::from(base).join(APP_DIR),
        _ => PathBuf::from(APP_DIR),
    }
}

/// The platform's own idea of where this app belongs.
#[cfg(not(windows))]
fn default_config_dir() -> PathBuf {
    if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
        if !base.is_empty() {
            return PathBuf::from(base).join(APP_DIR);
        }
    }
    match env::var_os("HOME") {
        Some(base) if !base.is_empty() => PathBuf::from(base).join(".config").join(APP_DIR),
        _ => PathBuf::from(APP_DIR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The environment is shared by every test in the process, so the tests
    /// that touch it take turns.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn the_environment_variable_wins() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let previous = env::var_os(CONFIG_DIR_ENV);
        env::set_var(CONFIG_DIR_ENV, "somewhere-else");
        assert_eq!(config_dir(), PathBuf::from("somewhere-else"));
        match previous {
            Some(value) => env::set_var(CONFIG_DIR_ENV, value),
            None => env::remove_var(CONFIG_DIR_ENV),
        }
    }

    #[test]
    fn the_bindings_file_lives_inside_the_configuration_directory() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let previous = env::var_os(CONFIG_DIR_ENV);
        env::set_var(CONFIG_DIR_ENV, "somewhere-else");
        assert_eq!(
            bindings_path(),
            PathBuf::from("somewhere-else").join(BINDINGS_FILE)
        );
        match previous {
            Some(value) => env::set_var(CONFIG_DIR_ENV, value),
            None => env::remove_var(CONFIG_DIR_ENV),
        }
    }

    #[test]
    fn without_the_override_the_folder_belongs_to_the_app() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let previous = env::var_os(CONFIG_DIR_ENV);
        env::remove_var(CONFIG_DIR_ENV);
        let dir = config_dir();
        if let Some(value) = previous {
            env::set_var(CONFIG_DIR_ENV, value);
        }
        assert!(dir.ends_with(APP_DIR), "{dir:?} should end with {APP_DIR}");
    }
}

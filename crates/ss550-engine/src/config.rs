//! Where the app keeps its files.

use std::env;
use std::path::{Path, PathBuf};

/// The environment variable that overrides the configuration directory.
///
/// It exists so the tests (and anyone debugging a broken install) can point
/// the app at a folder of their own.
pub const CONFIG_DIR_ENV: &str = "SS550_CONFIG_DIR";

/// The environment variable that keeps the window from looking for a deck.
///
/// Set it to any value and the window opens, renders and never opens a device.
/// It exists for two reasons: a timing run cannot paint the deck on the desk,
/// and a support report can be produced on a machine with no hardware. It is
/// read once, when the window is built, and never written by the app.
pub const NO_DECK_ENV: &str = "DECK_CONTROLLER_NO_DECK";

/// The folder the app owns inside the user's configuration.
const APP_DIR: &str = "ss550";

/// The file the bindings lived in before they were stored per deck.
const BINDINGS_FILE: &str = "bindings.json";

/// The folder the per-deck bindings live in.
const BINDINGS_DIR: &str = "bindings";

/// The folder each deck's profiles live in.
const PROFILES_DIR: &str = "profiles";

/// The folder each deck's recently used actions live in.
const RECENTS_DIR: &str = "recents";

/// The configuration directory for this app.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = env::var_os(CONFIG_DIR_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    default_config_dir()
}

/// Where the bindings file lived before M4: one file for every deck.
///
/// It is still read, so an existing install keeps its bindings, but new writes
/// go to [`bindings_path_for`] with the connected deck's own name.
pub fn bindings_path() -> PathBuf {
    config_dir().join(BINDINGS_FILE)
}

/// Where the bindings for one deck live.
///
/// The deck key is the deck's identity when it reports one, or its shape when
/// it does not; the window is the one that builds it. This function only turns
/// that key into a stable, filesystem-safe file inside the app's own folder, so
/// two decks never share a bindings file and a deck key can never escape the
/// folder with a path separator.
///
/// ```
/// use ss550_engine::config::bindings_path_for;
///
/// let first = bindings_path_for("deck one");
/// let second = bindings_path_for("deck two");
/// assert_ne!(first, second);
/// assert_eq!(first.parent().and_then(|path| path.file_name()), Some(std::ffi::OsStr::new("bindings")));
/// ```
pub fn bindings_path_for(deck_key: &str) -> PathBuf {
    config_dir()
        .join(BINDINGS_DIR)
        .join(format!("{}.json", file_stem(deck_key)))
}

/// Where one deck's profiles live.
///
/// The name is built the same way the bindings file's is, and for a better
/// reason than tidiness: a deck key contains `:` and `·`, and a `:` in a Windows
/// path is not a character, it is a separator between a file and an alternate
/// data stream. Writing `profiles/shape: 15 keys….json` therefore created a file
/// called `shape` and hid the profiles inside one of its streams, where the next
/// key change could not find them.
///
/// # Example
///
/// ```
/// let first = ss550_engine::config::profiles_path_for("deck one");
/// let second = ss550_engine::config::profiles_path_for("deck two");
/// assert_ne!(first, second);
/// assert_eq!(
///     first.parent().and_then(|path| path.file_name()),
///     Some(std::ffi::OsStr::new("profiles"))
/// );
/// ```
pub fn profiles_path_for(deck_key: &str) -> PathBuf {
    profiles_path_in(&config_dir(), deck_key)
}

/// Where one deck's recently used actions live.
///
/// Built like the bindings and the profiles, for the same reason: the deck key
/// contains characters a file name cannot hold. Keeping the helper here means
/// the window cannot invent a second spelling of the same key and scatter one
/// deck's data across two files.
///
/// # Example
///
/// ```
/// let first = ss550_engine::config::recents_path_for("deck one");
/// let second = ss550_engine::config::recents_path_for("deck two");
/// assert_ne!(first, second);
/// assert_eq!(
///     first.parent().and_then(|path| path.file_name()),
///     Some(std::ffi::OsStr::new("recents"))
/// );
/// ```
pub fn recents_path_for(deck_key: &str) -> PathBuf {
    recents_path_in(&config_dir(), deck_key)
}

/// [`recents_path_for`], inside an explicit root.
pub(crate) fn recents_path_in(root: &Path, deck_key: &str) -> PathBuf {
    root.join(RECENTS_DIR)
        .join(format!("{}.json", file_stem(deck_key)))
}

/// [`profiles_path_for`], inside an explicit root.
///
/// The store takes the configuration folder as an argument so a test can point
/// it at a scratch folder; this keeps the file naming in one place anyway.
pub(crate) fn profiles_path_in(root: &Path, deck_key: &str) -> PathBuf {
    root.join(PROFILES_DIR)
        .join(format!("{}.json", file_stem(deck_key)))
}

/// Reads a JSON file, ignoring a UTF-8 byte-order mark.
///
/// A BOM is legal UTF-8 and every Windows editor that offers "UTF-8" in an
/// older PowerShell writes one. `serde_json` refuses it at byte zero, so without
/// this a hand-edited bindings or profile file would read as "not valid JSON"
/// and the user's data would look lost. Dropping it costs one comparison.
pub(crate) fn read_json(path: &Path) -> std::io::Result<String> {
    let text = std::fs::read_to_string(path)?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    })
}

/// A stable file name for a deck key: a readable slug plus a fingerprint.
///
/// The fingerprint is what keeps two decks apart when their slugs collide, and
/// what makes the name stable when the key contains characters a file system
/// would not accept.
fn file_stem(deck_key: &str) -> String {
    let mut slug = String::new();
    for character in deck_key.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "deck" } else { slug };
    let slug: String = slug.chars().take(40).collect();
    format!("{slug}-{:016x}", fingerprint(deck_key))
}

/// FNV-1a, 64-bit: no dependency, stable across releases, good enough to keep
/// two deck keys in two files.
fn fingerprint(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
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

    /// Runs `body` with a known configuration directory, then restores it.
    fn with_config_dir(body: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let previous = env::var_os(CONFIG_DIR_ENV);
        env::set_var(CONFIG_DIR_ENV, "somewhere-else");
        body();
        match previous {
            Some(value) => env::set_var(CONFIG_DIR_ENV, value),
            None => env::remove_var(CONFIG_DIR_ENV),
        }
    }

    #[test]
    fn two_decks_get_two_files() {
        with_config_dir(|| {
            assert_ne!(
                bindings_path_for("deck-simulator: 6 keys, 2 dials"),
                bindings_path_for("shape: 8 keys, 4 columns, 72x72")
            );
        });
    }

    #[test]
    fn the_same_deck_gets_the_same_file() {
        with_config_dir(|| {
            assert_eq!(
                bindings_path_for("deck-simulator: 6 keys, 2 dials"),
                bindings_path_for("deck-simulator: 6 keys, 2 dials")
            );
        });
    }

    #[test]
    fn a_deck_key_cannot_escape_the_bindings_folder() {
        with_config_dir(|| {
            let root = PathBuf::from("somewhere-else");
            let folder = root.join(BINDINGS_DIR);
            let path = bindings_path_for("../../another-folder/deck");
            assert!(
                path.starts_with(&folder),
                "{path:?} left the app's own folder"
            );
            assert_eq!(path.parent(), Some(folder.as_path()));
        });
    }

    #[test]
    fn the_legacy_file_is_still_the_one_at_the_top() {
        with_config_dir(|| {
            assert_eq!(
                bindings_path(),
                PathBuf::from("somewhere-else").join(BINDINGS_FILE)
            );
            assert_ne!(bindings_path(), bindings_path_for("any deck"));
        });
    }
}

//! # ss550-discord
//!
//! The Discord client's local pipe, kept as the record of a closed door.
//!
//! ## What works
//!
//! The Discord client listens on `\\.\pipe\discord-ipc-{0..9}` and speaks a frame
//! format of its own. [`pipe`] finds it and frames it; [`protocol`] builds the
//! three payloads that matter and reads the answers. All of that is measured
//! against the real client, and opening the pipe needs no `unsafe`: [`std::fs::File`]
//! opens `\\.\pipe\<name>` through `CreateFileW`.
//!
//! ## What does not, and why it is written down here
//!
//! A key that *mutes the microphone* and *shows whether it is muted* cannot be
//! built against Discord's API by this project. Measured, on a real client:
//!
//! - `SET_VOICE_SETTINGS_2` and the `VOICE_SETTINGS_UPDATE_2` event are in the
//!   `rpc.local` scope, so any local connection may call them — but they are an
//!   **application-private sandbox**. The command changes nothing in the client,
//!   and the event reports nothing about it. With the user muting and unmuting in
//!   Discord's own window for two minutes, the only event that arrived was the
//!   initial snapshot.
//! - The commands that *do* work — `SET_VOICE_SETTINGS`, `GET_VOICE_SETTINGS`,
//!   the `VOICE_SETTINGS_UPDATE` event — need `rpc.voice.read` / `rpc.voice.write`.
//!   `AUTHORIZE` answers `5000 invalid_scope` for them, while the same call with
//!   `identify` grants consent and authenticates; and even authenticated,
//!   `GET_VOICE_SETTINGS` answers `4006 Not authenticated or invalid scope`.
//!   Discord reserves those scopes for applications with partner approval.
//!
//! A key that painted the sandbox state as if it were the microphone would be
//! lying about a live microphone, which is the one thing this project does not
//! do. So the surface was removed, and what remains is the part that is true.
//!
//! ## What to use instead
//!
//! [`ss550_engine::Action::Keys`] sending Discord's own global bind — `ctrl+shift+m`
//! for the microphone, `ctrl+shift+d` for deafen. It mutes for real, it needs no
//! grant, and it cannot show the state, because Discord will not state it.
//!
//! Rich Presence is still open: `SET_ACTIVITY` is `rpc.local` and works without
//! any grant, so publishing what the deck is doing remains possible. It is not
//! built.
//!
//! ## The probes
//!
//! ```text
//! cargo run -p ss550-discord --example voice_probe -- <application id> 20
//! cargo run -p ss550-discord --example voice_raw   -- <application id>
//! $env:DISCORD_CLIENT_SECRET = "…"; cargo run -p ss550-discord --example voice_oauth -- <application id>
//! ```
//!
//! `voice_probe` prints every frame the pipe carries. `voice_raw` sends the real
//! voice command and prints Discord's refusal. `voice_oauth` walks the whole
//! authorization flow and shows where it stops.

#![deny(unsafe_code)]

pub mod pipe;
pub mod protocol;

use std::fs;
use std::io;
use std::path::PathBuf;

use ss550_engine::config_dir;

/// The file the Application ID is kept in, inside the configuration folder.
pub const DISCORD_FILE: &str = "discord.json";

/// The key the Application ID is stored under.
const APPLICATION_ID: &str = "application_id";

/// The lowest number of digits a Discord snowflake has had since 2015.
const MIN_ID_DIGITS: usize = 17;

/// The most digits a Discord snowflake has.
const MAX_ID_DIGITS: usize = 20;

/// The file the Application ID is read from.
pub fn config_path() -> PathBuf {
    config_dir().join(DISCORD_FILE)
}

/// Whether a string could be a Discord application's id.
///
/// An application id is a snowflake: digits only, and long. Checking it here
/// means a pasted ".env" line is refused before it arrives at Discord as a
/// malformed handshake that only says `invalid client id`.
///
/// ```
/// use ss550_discord::looks_like_application_id;
///
/// assert!(looks_like_application_id("1093449380133974058"));
/// assert!(!looks_like_application_id(""));
/// assert!(!looks_like_application_id("client_id=123"));
/// assert!(!looks_like_application_id("12345"));
/// ```
pub fn looks_like_application_id(text: &str) -> bool {
    let trimmed = text.trim();
    (MIN_ID_DIGITS..=MAX_ID_DIGITS).contains(&trimmed.len())
        && trimmed.chars().all(|character| character.is_ascii_digit())
}

/// The Application ID stored on disk, when there is a usable one.
pub fn application_id() -> Option<String> {
    let text = fs::read_to_string(config_path()).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let id = value.get(APPLICATION_ID)?.as_str()?.trim().to_string();
    looks_like_application_id(&id).then_some(id)
}

/// Stores the Application ID, or clears it when given nothing usable.
///
/// Returns the value that is now in force, so a caller can tell the user what
/// happened without reading the file back.
pub fn save_application_id(text: Option<&str>) -> Result<Option<String>, io::Error> {
    let cleaned = text.map(str::trim).filter(|id| !id.is_empty());
    match cleaned {
        None => match fs::remove_file(config_path()) {
            // Nothing stored is the state that was asked for.
            Ok(()) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        },
        Some(id) if !looks_like_application_id(id) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a Discord Application ID is 17 to 20 digits and nothing else",
        )),
        Some(id) => {
            let path = config_path();
            if let Some(folder) = path.parent() {
                fs::create_dir_all(folder)?;
            }
            let body = serde_json::json!({ APPLICATION_ID: id }).to_string();
            fs::write(&path, body)?;
            Ok(Some(id.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss550_engine::config::CONFIG_DIR_ENV;

    /// Points the configuration folder at a scratch directory for one test.
    ///
    /// The variable is process-wide, so every test that writes a file takes the
    /// same lock and restores what it found.
    struct Scratch {
        _guard: std::sync::MutexGuard<'static, ()>,
        folder: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl Scratch {
        fn new() -> Self {
            let guard = LOCK.lock().expect("the scratch lock");
            let folder = tempfile::tempdir().expect("a scratch folder");
            let previous = std::env::var_os(CONFIG_DIR_ENV);
            std::env::set_var(CONFIG_DIR_ENV, folder.path());
            Self {
                _guard: guard,
                folder,
                previous,
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(CONFIG_DIR_ENV, value),
                None => std::env::remove_var(CONFIG_DIR_ENV),
            }
            let _ = &self.folder;
        }
    }

    #[test]
    fn an_application_id_is_digits_only_and_long_enough() {
        assert!(looks_like_application_id("1093449380133974058"));
        assert!(looks_like_application_id(" 1093449380133974058 "));
        assert!(!looks_like_application_id(""));
        assert!(!looks_like_application_id("1093449380133974"));
        assert!(!looks_like_application_id("10934493801339740580000"));
        assert!(!looks_like_application_id("109344938013397405a"));
        assert!(!looks_like_application_id(
            "DISCORD_APP_ID=1093449380133974058"
        ));
    }

    #[test]
    fn a_stored_id_is_read_back_trimmed() {
        let _scratch = Scratch::new();
        let stored = save_application_id(Some(" 1093449380133974058 "))
            .expect("the file is writable")
            .expect("an id is in force");
        assert_eq!(stored, "1093449380133974058");
        assert_eq!(application_id().as_deref(), Some("1093449380133974058"));
    }

    #[test]
    fn a_malformed_id_is_refused_before_it_reaches_the_disk() {
        let _scratch = Scratch::new();
        let error = save_application_id(Some("client_id=123")).expect_err("refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(application_id(), None, "nothing was written");
    }

    #[test]
    fn clearing_the_id_leaves_no_file_behind() {
        let _scratch = Scratch::new();
        save_application_id(Some("1093449380133974058")).expect("writable");
        assert!(config_path().exists());

        assert_eq!(save_application_id(None).expect("removable"), None);
        assert!(!config_path().exists(), "a cleared id leaves no file");
        assert_eq!(application_id(), None);

        // Clearing twice is the state that was asked for, not an error.
        assert_eq!(save_application_id(None).expect("still gone"), None);
    }

    #[test]
    fn an_empty_field_clears_instead_of_storing_an_empty_id() {
        let _scratch = Scratch::new();
        save_application_id(Some("1093449380133974058")).expect("writable");
        assert_eq!(save_application_id(Some("   ")).expect("cleared"), None);
        assert_eq!(application_id(), None);
    }
}

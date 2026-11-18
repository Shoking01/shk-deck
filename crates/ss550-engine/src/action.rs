//! The actions a key can perform.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ss550_plugin_api::{CellId, PluginId};

use crate::keys::KeyCombo;
use crate::profiles::MAX_PAGE_NAME_CHARS;
use crate::{ActionError, KeyComboError};

/// What a key does when it is pressed.
///
/// The catalogue is deliberately small — a program to run, a URL to open, or a
/// keystroke to send — because every extra kind is a promise to keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Runs a program.
    Launch {
        /// Path to the executable.
        program: PathBuf,
        /// Arguments for the program, each one a separate argv entry.
        ///
        /// They are never re-joined into a command line, so an argument can
        /// never turn into a second command.
        #[serde(default)]
        args: Vec<String>,
    },
    /// Opens a URL with the system's default handler.
    OpenUrl {
        /// The URL, scheme included.
        url: String,
    },
    /// Sends a key combination to the window that has the focus.
    ///
    /// The combination is stored and written as its canonical spelling,
    /// `ctrl+shift+m`, so the JSON stays one string that reads the way it was
    /// meant; a [`KeyCombo`] cannot hold a combination that does not exist, so
    /// an action of this kind is always valid to build.
    ///
    /// ```
    /// use ss550_engine::Action;
    ///
    /// let action = Action::keys("Ctrl+Shift+M").expect("valid combination");
    /// assert_eq!(action.summary(), "Ctrl+Shift+M");
    ///
    /// let json = serde_json::to_string(&action).expect("serialise");
    /// assert_eq!(json, r#"{"type":"keys","keys":"ctrl+shift+m"}"#);
    /// ```
    Keys {
        /// The combination to send.
        keys: KeyCombo,
    },
    /// Shows another page of this profile: what a folder is on the deck.
    ///
    /// Navigation is not an action that runs anywhere: it changes what the deck
    /// is showing, so the window applies it and the runner refuses it. It is
    /// stored like any other action, which is what lets a key be moved, copied
    /// and saved with the profile it belongs to.
    Folder {
        /// The page's name, exactly as the profile spells it.
        page: String,
    },
    /// Goes back to the page the deck came from.
    ///
    /// The window keeps the way in; this is the way out. On the root there is
    /// nowhere to go, and the window says so.
    Back,
    /// Activates another profile of this deck.
    Profile {
        /// The profile's name, exactly as the store spells it.
        name: String,
    },
    /// Shows a live reading from a plugin on this key.
    ///
    /// The key becomes a gauge: the window draws the plugin's reading into the
    /// key's picture and keeps it up to date, so pressing it runs nothing. It is
    /// stored like any other action, which is what lets a key be moved, copied
    /// and saved with the profile it belongs to.
    ///
    /// ```
    /// use ss550_engine::Action;
    ///
    /// let action = Action::Plugin {
    ///     plugin: "system-stats".to_string(),
    ///     cell: "cpu.usage".to_string(),
    /// };
    /// assert!(action.validate().is_ok());
    /// assert_eq!(
    ///     serde_json::to_string(&action).expect("serialise"),
    ///     r#"{"type":"plugin","plugin":"system-stats","cell":"cpu.usage"}"#
    /// );
    /// ```
    Plugin {
        /// The plugin's id, e.g. `system-stats`.
        plugin: String,
        /// The cell inside that plugin, e.g. `cpu.usage`.
        cell: String,
    },
}

impl Action {
    /// Binds a key combination, checking it as it is built.
    ///
    /// This is the constructor the editor calls: a combination that does not
    /// exist never becomes an action, so there is no later moment where a
    /// broken keystroke can reach the deck.
    ///
    /// ```
    /// use ss550_engine::Action;
    /// use ss550_engine::KeyComboError;
    ///
    /// assert!(Action::keys("ctrl+shift+m").is_ok());
    /// assert!(matches!(Action::keys("ctrl+shift"), Err(KeyComboError::NoKey { .. })));
    /// ```
    pub fn keys(text: &str) -> Result<Self, KeyComboError> {
        KeyCombo::parse(text).map(|keys| Action::Keys { keys })
    }

    /// Checks that this action can run.
    ///
    /// ```
    /// use ss550_engine::Action;
    ///
    /// let action = Action::OpenUrl { url: "https://example.com".to_string() };
    /// assert!(action.validate().is_ok());
    ///
    /// // A key combination cannot be built broken: the parser is the check,
    /// // and what it lets through is valid.
    /// let keys = Action::keys("ctrl+shift+m").expect("valid combination");
    /// assert!(keys.validate().is_ok());
    /// ```
    pub fn validate(&self) -> Result<(), ActionError> {
        match self {
            Action::Launch { program, .. } => {
                if program.as_os_str().is_empty() {
                    Err(ActionError::EmptyProgram)
                } else {
                    Ok(())
                }
            }
            Action::OpenUrl { url } => validate_url(url),
            // A `KeyCombo` is proof that the combination parses, and its fields
            // are private, so there is nothing left to check here.
            Action::Keys { .. } => Ok(()),
            Action::Folder { page } => Self::validate_page_name(page),
            Action::Back => Ok(()),
            Action::Profile { name } => Self::validate_profile_name(name),
            Action::Plugin { plugin, cell } => Self::validate_plugin_cell(plugin, cell),
        }
    }

    /// A copy with accidental wrapping quotes removed.
    ///
    /// Explorer's *Copy as path* wraps the path in double quotes, so a pasted
    /// path almost always arrives as `"C:\…\app.exe"`. Those quotes are **not**
    /// part of the name: `CreateProcess` rejects them with
    /// `ERROR_INVALID_NAME` (os error 123), which reads as a broken path even
    /// though the path is perfect. A pasted URL has the same problem.
    ///
    /// Only a **matching** leading and trailing quote is removed. A lone quote
    /// is left alone, so a genuinely malformed path still reports its real
    /// error instead of being silently guessed at.
    ///
    /// ```
    /// use ss550_engine::Action;
    ///
    /// let pasted = Action::Launch {
    ///     program: "\"C:\\Windows\\notepad.exe\"".into(),
    ///     args: Vec::new(),
    /// };
    /// let action = pasted.without_wrapping_quotes();
    /// let Action::Launch { program, .. } = action else { unreachable!() };
    /// assert_eq!(program.to_str(), Some("C:\\Windows\\notepad.exe"));
    /// ```
    pub fn without_wrapping_quotes(&self) -> Self {
        match self {
            Action::Launch { program, args } => {
                let text = program.to_string_lossy();
                let cleaned = strip_wrapping_quotes(&text);
                // Keep the original value when nothing was stripped, so a
                // non-UTF-8 path is never rewritten through a lossy string.
                let program = if cleaned.len() == text.len() && cleaned == text.as_ref() {
                    program.clone()
                } else {
                    PathBuf::from(cleaned)
                };
                Action::Launch {
                    program,
                    args: args.clone(),
                }
            }
            Action::OpenUrl { url } => Action::OpenUrl {
                url: strip_wrapping_quotes(url).to_string(),
            },
            // A combination has no quotes to strip. The signature promises a
            // new action, so the combination is copied whole; a `Cow<Action>`
            // would save that copy and force every caller to deal with a type
            // it does not need.
            Action::Keys { keys } => Action::Keys { keys: keys.clone() },
            // Navigation carries a name and nothing to clean: a name is trimmed
            // when it is validated, and rewriting it here would give one page
            // two spellings.
            Action::Folder { page } => Action::Folder { page: page.clone() },
            Action::Back => Action::Back,
            Action::Profile { name } => Action::Profile { name: name.clone() },
            // A plugin cell is addressed by its exact spelling, so a pasted or
            // padded id is cleaned here: ` cpu.usage ` is not a cell, and a
            // binding that cannot match the catalogue is a key that never
            // updates.
            Action::Plugin { plugin, cell } => Action::Plugin {
                plugin: plugin.trim().to_string(),
                cell: cell.trim().to_string(),
            },
        }
    }

    /// Checks that a page name can address a page this build will accept.
    ///
    /// The rule is the profile's, stated once: a name that survives trimming has to
    /// be non-empty, no longer than [`MAX_PAGE_NAME_CHARS`] and free of control
    /// characters, because a page is addressed by its name and a name nothing can
    /// type is a page nothing can open.
    fn validate_page_name(page: &str) -> Result<(), ActionError> {
        let trimmed = page.trim();
        if trimmed.is_empty() {
            return Err(ActionError::EmptyPageName);
        }
        if trimmed.chars().count() > MAX_PAGE_NAME_CHARS {
            return Err(ActionError::PageNameTooLong {
                max: MAX_PAGE_NAME_CHARS,
            });
        }
        if trimmed.chars().any(char::is_control) {
            return Err(ActionError::PageNameControlCharacter);
        }
        Ok(())
    }

    /// Checks that a profile name is something the store could hold.
    pub(crate) fn validate_profile_name(name: &str) -> Result<(), ActionError> {
        match name.trim().is_empty() {
            true => Err(ActionError::EmptyProfileName),
            false => Ok(()),
        }
    }

    /// Checks that a plugin binding names two ids the contract accepts.
    ///
    /// The spelling rule is the plugin contract's own — `ss550-plugin-api`
    /// states it once — so a binding this build writes and a plugin a later
    /// loader provides cannot disagree about what a cell is called.
    fn validate_plugin_cell(plugin: &str, cell: &str) -> Result<(), ActionError> {
        PluginId::parse(plugin).map_err(|source| ActionError::PluginId { source })?;
        CellId::parse(cell).map_err(|source| ActionError::PluginCell { source })?;
        Ok(())
    }

    /// A one-line label for the key tiles: what the key does, briefly.
    ///
    /// A combination shows itself, because that is the whole of what it does:
    /// the tile reads `Ctrl+Shift+M`, and [`MAX_SUMMARY_CHARS`] is the bound the
    /// vocabulary can produce, so a tile never has to cut one in half.
    ///
    /// ```
    /// use ss550_engine::Action;
    ///
    /// let action = Action::OpenUrl { url: "https://twitch.tv/dashboard".to_string() };
    /// assert_eq!(action.summary(), "OPEN twitch.tv");
    ///
    /// let action = Action::Launch {
    ///     program: "C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe".into(),
    ///     args: Vec::new(),
    /// };
    /// assert_eq!(action.summary(), "LAUNCH obs64.exe");
    ///
    /// let action = Action::keys("ctrl+shift+m").expect("valid combination");
    /// assert_eq!(action.summary(), "Ctrl+Shift+M");
    /// ```
    ///
    /// [`MAX_SUMMARY_CHARS`]: crate::keys::MAX_SUMMARY_CHARS
    pub fn summary(&self) -> String {
        match self {
            Action::Launch { program, .. } => format!("LAUNCH {}", file_label(program)),
            Action::OpenUrl { url } => format!("OPEN {}", host_of(url)),
            Action::Keys { keys } => keys.to_string(),
            Action::Folder { page } => format!("FOLDER {}", page.to_uppercase()),
            Action::Back => "BACK".to_string(),
            Action::Profile { name } => format!("PROFILE {}", name.to_uppercase()),
            // The key's picture is the reading, so the one-line label names the
            // measurement and not the plugin: `STAT cpu.usage` is what the
            // status line and the recents list have room for.
            Action::Plugin { cell, .. } => format!("STAT {cell}"),
        }
    }

    /// Whether this action changes what the deck shows instead of running
    /// something.
    ///
    /// The window asks before handing an action to the runner: navigation is
    /// applied where the keys are drawn, because the window owns the page stack,
    /// and handing it to a worker would give two threads a claim on the same
    /// state.
    pub fn is_navigation(&self) -> bool {
        matches!(
            self,
            Action::Folder { .. } | Action::Back | Action::Profile { .. }
        )
    }

    /// Whether this action is a plugin reading rather than something to run.
    ///
    /// A reading is drawn on the key and refreshed by the window; nothing is
    /// launched when the key is pressed, so the runner is never given one.
    pub fn is_reading(&self) -> bool {
        matches!(self, Action::Plugin { .. })
    }

    /// The plugin and cell this action names, when it names one.
    pub fn plugin_cell(&self) -> Option<(&str, &str)> {
        match self {
            Action::Plugin { plugin, cell } => Some((plugin, cell)),
            _ => None,
        }
    }

    /// The full description, for the status line.
    pub fn describe(&self) -> String {
        match self {
            Action::Launch { program, args } => {
                if args.is_empty() {
                    format!("Launch {}", program.display())
                } else {
                    format!("Launch {} {}", program.display(), args.join(" "))
                }
            }
            Action::OpenUrl { url } => format!("Open {url}"),
            Action::Keys { keys } => format!("Send {keys}"),
            Action::Folder { page } => format!("Open the page {page}"),
            Action::Back => "Go back to the page this one was opened from".to_string(),
            Action::Profile { name } => format!("Switch to the profile {name}"),
            Action::Plugin { plugin, cell } => {
                format!("Show {cell}, measured by the plugin {plugin}")
            }
        }
    }
}

/// Checks that a string is a URL this app is willing to open.
///
/// A URL needs a scheme (`https://`, `mailto:`, `steam:` …). A bare string is
/// rejected instead of being guessed at: guessing is how a path turns into
/// something the user never asked to open.
///
/// ```
/// use ss550_engine::action::validate_url;
///
/// assert!(validate_url("https://example.com").is_ok());
/// assert!(validate_url("mailto:someone@example.com").is_ok());
/// assert!(validate_url("example.com").is_err());
/// ```
pub fn validate_url(url: &str) -> Result<(), ActionError> {
    let trimmed = url.trim();

    let Some(colon) = trimmed.find(':') else {
        return Err(ActionError::InvalidUrl(url.to_string()));
    };
    let (scheme, rest) = trimmed.split_at(colon);

    // A one-letter scheme is a Windows drive letter (`C:\...`), not a URI
    // scheme, so a scheme needs at least two characters.
    let scheme_ok = scheme.len() >= 2
        && scheme
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !scheme_ok {
        return Err(ActionError::InvalidUrl(url.to_string()));
    }

    // Something has to follow the colon: "https:" alone is not a URL, and
    // "https://" has no host to open.
    let rest = &rest[1..];
    if rest.is_empty() {
        return Err(ActionError::InvalidUrl(url.to_string()));
    }
    if let Some(authority) = rest.strip_prefix("//") {
        let host = authority.split(['/', '?', '#']).next().unwrap_or_default();
        if host.is_empty() {
            return Err(ActionError::InvalidUrl(url.to_string()));
        }
    }

    // Whitespace and control characters are never valid in a URI and would
    // break the command line they end up on.
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ActionError::InvalidUrl(url.to_string()));
    }

    Ok(())
}

/// The last segment of a stored path, whichever separator it uses.
///
/// [`Path::file_name`] splits on the host's separator only, so a Windows path
/// kept in the bindings file renders as one long segment when this crate is
/// built for another platform. A key tile is display data: the same binding
/// has to read the same on every machine, so both separators count.
///
/// A value that is nothing but separators keeps its whole text rather than
/// disappearing from the tile.
pub(crate) fn file_label(program: &Path) -> String {
    let text = program.to_string_lossy();
    match text.rsplit(['\\', '/']).find(|segment| !segment.is_empty()) {
        Some(segment) => segment.to_string(),
        None => text.into_owned(),
    }
}

/// The host part of a URL, for the short label.
fn host_of(url: &str) -> &str {
    let without_scheme = match url.find("://") {
        Some(index) => &url[index + 3..],
        None => url,
    };
    without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme)
}

/// Removes one matching pair of wrapping double quotes, plus surrounding
/// whitespace.
///
/// A lone quote is kept: it is a real defect in the value, and hiding it would
/// turn a clear error into a wrong path.
fn strip_wrapping_quotes(text: &str) -> &str {
    let trimmed = text.trim();
    match trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(inner) => inner.trim(),
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_urls_with_a_scheme() {
        for url in [
            "https://example.com",
            "http://example.com/path?query=1",
            "mailto:someone@example.com",
            "steam://run/440",
            "https://example.com:8443/x",
        ] {
            assert!(validate_url(url).is_ok(), "{url} should be accepted");
        }
    }

    #[test]
    fn rejects_strings_that_are_not_urls() {
        for url in [
            "",
            "   ",
            "example.com",
            "C:\\Windows\\notepad.exe",
            "C:/Windows/notepad.exe",
            "turn off",
            "://example.com",
            "1http://example.com",
            "https://",
            "https://example .com",
            "https://exa\tmple.com",
        ] {
            assert!(validate_url(url).is_err(), "{url:?} should be rejected");
        }
    }

    #[test]
    fn a_launch_action_needs_a_program() {
        let empty = Action::Launch {
            program: PathBuf::new(),
            args: Vec::new(),
        };
        assert!(matches!(empty.validate(), Err(ActionError::EmptyProgram)));

        let valid = Action::Launch {
            program: PathBuf::from("notepad.exe"),
            args: Vec::new(),
        };
        assert!(valid.validate().is_ok());
    }

    #[test]
    fn a_pasted_path_loses_its_wrapping_quotes() {
        let pasted = Action::Launch {
            program: "\"C:\\Users\\me\\App\\app.exe\"".into(),
            args: vec!["--profile".to_string(), "Live".to_string()],
        };
        let Action::Launch { program, args } = pasted.without_wrapping_quotes() else {
            panic!("still a launch");
        };
        assert_eq!(program.to_str(), Some("C:\\Users\\me\\App\\app.exe"));
        assert_eq!(args, vec!["--profile".to_string(), "Live".to_string()]);
    }

    #[test]
    fn a_path_without_quotes_is_left_alone() {
        let plain = Action::Launch {
            program: "C:\\Windows\\notepad.exe".into(),
            args: Vec::new(),
        };
        let Action::Launch { program, .. } = plain.without_wrapping_quotes() else {
            panic!("still a launch");
        };
        assert_eq!(program.to_str(), Some("C:\\Windows\\notepad.exe"));
    }

    #[test]
    fn a_lone_quote_is_not_hidden() {
        for text in ["\"C:\\Windows\\notepad.exe", "C:\\Windows\\notepad.exe\""] {
            let action = Action::Launch {
                program: text.into(),
                args: Vec::new(),
            };
            let Action::Launch { program, .. } = action.without_wrapping_quotes() else {
                panic!("still a launch");
            };
            assert_eq!(
                program.to_str(),
                Some(text),
                "an unmatched quote is a real defect and must stay visible"
            );
        }
    }

    #[test]
    fn a_quoted_url_loses_its_quotes() {
        let pasted = Action::OpenUrl {
            url: "\"https://example.com\"".to_string(),
        };
        assert_eq!(
            pasted.without_wrapping_quotes(),
            Action::OpenUrl {
                url: "https://example.com".to_string()
            }
        );
    }

    #[test]
    fn the_navigation_actions_have_a_shape_in_the_file() {
        // These are written into the bindings file, so the spelling is a
        // contract: a build downstream reads what this one writes.
        assert_eq!(
            serde_json::to_string(&Action::Folder {
                page: "Media".to_string()
            })
            .expect("serialise"),
            r#"{"type":"folder","page":"Media"}"#
        );
        assert_eq!(
            serde_json::to_string(&Action::Back).expect("serialise"),
            r#"{"type":"back"}"#
        );
        assert_eq!(
            serde_json::to_string(&Action::Profile {
                name: "stream".to_string()
            })
            .expect("serialise"),
            r#"{"type":"profile","name":"stream"}"#
        );
    }

    #[test]
    fn a_key_can_hold_a_navigation_action() {
        // S2: the three are bound like any other action, and each says whether
        // it changes the deck instead of running something.
        for action in [
            Action::Folder {
                page: "Media".to_string(),
            },
            Action::Back,
            Action::Profile {
                name: "stream".to_string(),
            },
        ] {
            let json = serde_json::to_string(&action).expect("serialise");
            let read: Action = serde_json::from_str(&json).expect("read back");
            assert_eq!(read, action);
            assert!(action.is_navigation(), "{json} is navigation");
            assert!(action.validate().is_ok(), "{json} is valid");
        }
        assert!(!Action::keys("ctrl+m")
            .expect("a combination")
            .is_navigation());
    }

    #[test]
    fn a_plugin_cell_is_stored_as_the_contract_spells_it() {
        // S7: the spelling is the contract's, and a padded id is cleaned rather
        // than stored as a key that can never match the catalogue.
        let typed = Action::Plugin {
            plugin: " system-stats ".to_string(),
            cell: " cpu.usage ".to_string(),
        };
        let stored = typed.without_wrapping_quotes();
        assert_eq!(
            stored,
            Action::Plugin {
                plugin: "system-stats".to_string(),
                cell: "cpu.usage".to_string(),
            }
        );
        let json = serde_json::to_string(&stored).expect("serialise");
        assert_eq!(
            json,
            r#"{"type":"plugin","plugin":"system-stats","cell":"cpu.usage"}"#
        );
        let read: Action = serde_json::from_str(&json).expect("read back");
        assert_eq!(read, stored);
        assert!(read.validate().is_ok());
        assert_eq!(read.summary(), "STAT cpu.usage");
        assert_eq!(read.plugin_cell(), Some(("system-stats", "cpu.usage")));
        assert!(read.is_reading());
        assert!(!read.is_navigation(), "a reading does not move the deck");
    }

    #[test]
    fn a_plugin_cell_that_is_not_two_usable_ids_is_refused() {
        for (plugin, cell) in [
            ("", "cpu.usage"),
            ("system-stats", ""),
            ("System Stats", "cpu.usage"),
            ("system-stats", "cpu..usage"),
            ("system/stats", "cpu.usage"),
        ] {
            let action = Action::Plugin {
                plugin: plugin.to_string(),
                cell: cell.to_string(),
            };
            assert!(
                action.validate().is_err(),
                "{plugin:?} / {cell:?} must not become a binding"
            );
        }
        assert!(Action::Plugin {
            plugin: "system-stats".to_string(),
            cell: "cpu.usage".to_string(),
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn the_runner_refuses_a_reading_instead_of_spawning_its_name() {
        let action = Action::Plugin {
            plugin: "system-stats".to_string(),
            cell: "cpu.usage".to_string(),
        };
        let error = crate::run_action(&action).expect_err("the runner refuses it");
        assert!(
            matches!(error, ActionError::ReadingIsNotRunnable { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("cpu.usage"));
    }

    #[test]
    fn the_runner_refuses_navigation_instead_of_half_running_it() {
        // The window applies navigation; a worker with no window must say so
        // rather than pretend the key did something.
        for action in [
            Action::Folder {
                page: "Media".to_string(),
            },
            Action::Back,
        ] {
            assert!(
                matches!(
                    crate::run_action(&action),
                    Err(ActionError::NavigationIsNotRunnable { .. })
                ),
                "{action:?} must be refused by the runner"
            );
        }
    }

    #[test]
    fn a_page_name_that_could_not_address_a_page_is_refused() {
        assert!(Action::Folder {
            page: String::new()
        }
        .validate()
        .is_err());
        assert!(Action::Folder {
            page: "   ".to_string()
        }
        .validate()
        .is_err());
        assert!(Action::Folder {
            page: "bad\ttab".to_string()
        }
        .validate()
        .is_err());
        assert!(
            Action::Folder {
                page: "x".repeat(MAX_PAGE_NAME_CHARS + 1)
            }
            .validate()
            .is_err(),
            "a name longer than the cap is refused"
        );
        assert!(Action::Profile {
            name: " ".to_string()
        }
        .validate()
        .is_err());
        assert!(Action::Profile {
            name: "stream".to_string()
        }
        .validate()
        .is_ok());
        assert!(Action::Back.validate().is_ok());
    }

    #[test]
    fn the_summary_fits_on_a_key_tile() {
        let launch = Action::Launch {
            program: PathBuf::from("C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe"),
            args: vec!["--profile".to_string(), "Live".to_string()],
        };
        assert_eq!(launch.summary(), "LAUNCH obs64.exe");

        let url = Action::OpenUrl {
            url: "https://twitch.tv/dashboard".to_string(),
        };
        assert_eq!(url.summary(), "OPEN twitch.tv");

        let keys = Action::keys("ctrl+shift+m").expect("valid combination");
        assert_eq!(keys.summary(), "Ctrl+Shift+M");
    }

    #[test]
    fn no_combination_summary_is_longer_than_a_tile_can_hold() {
        // The worst case the vocabulary allows: every modifier and the longest
        // key name. The bound is what keeps a tile from cutting a combination
        // in half, so it has to hold for everything the parser accepts.
        let longest = Action::keys("ctrl+alt+shift+win+backspace").expect("valid combination");
        assert_eq!(longest.summary(), "Ctrl+Alt+Shift+Win+Backspace");
        assert_eq!(
            longest.summary().chars().count(),
            crate::keys::MAX_SUMMARY_CHARS
        );

        for text in [
            "m",
            "ctrl+f24",
            "win+pagedown",
            "ctrl+shift+escape",
            "ctrl+`",
            "alt+insert",
        ] {
            let action = Action::keys(text).expect("valid combination");
            assert!(
                action.summary().chars().count() <= crate::keys::MAX_SUMMARY_CHARS,
                "{text:?} must fit on a tile: {}",
                action.summary()
            );
        }
    }

    #[test]
    fn a_key_combination_describes_itself_for_the_status_line() {
        let keys = Action::keys("ctrl+shift+m").expect("valid combination");
        assert_eq!(keys.describe(), "Send Ctrl+Shift+M");

        // The same voice as the other two: a verb and what it acts on.
        assert!(Action::keys("f5")
            .expect("valid combination")
            .describe()
            .starts_with("Send "));
    }

    #[test]
    fn a_key_combination_round_trips_through_json() {
        let action = Action::keys("Shift+Ctrl+M").expect("valid combination");
        let json = serde_json::to_string(&action).expect("serialise");
        assert_eq!(json, r#"{"type":"keys","keys":"ctrl+shift+m"}"#);

        let restored: Action = serde_json::from_str(&json).expect("parse");
        assert_eq!(restored, action);
    }

    #[test]
    fn a_hand_written_combination_loads_in_any_spelling() {
        let file = r#"{"type":"keys","keys":"CTRL + Shift + m"}"#;
        let action: Action = serde_json::from_str(file).expect("parse");
        assert_eq!(action, Action::keys("ctrl+shift+m").expect("valid"));

        // And what was written back is the canonical spelling.
        let json = serde_json::to_string(&action).expect("serialise");
        assert_eq!(json, r#"{"type":"keys","keys":"ctrl+shift+m"}"#);
    }

    #[test]
    fn a_combination_that_does_not_exist_never_loads_as_an_action() {
        let file = r#"{"type":"keys","keys":"ctrl+shift"}"#;
        let error = serde_json::from_str::<Action>(file).expect_err("two modifiers are not a key");
        assert!(
            error.to_string().contains("no key"),
            "the parser's reason must reach the reader: {error}"
        );
    }

    #[test]
    fn a_built_key_action_is_always_valid() {
        // `KeyCombo` is the proof: validation cannot fail for this kind.
        for text in [
            "m",
            "ctrl+shift+m",
            "win+f24",
            "ctrl+alt+shift+win+pagedown",
        ] {
            assert!(
                Action::keys(text)
                    .expect("valid combination")
                    .validate()
                    .is_ok(),
                "for {text}"
            );
        }
    }

    #[test]
    fn a_tile_shows_the_last_segment_of_either_kind_of_path() {
        // A binding written on Windows is display data everywhere: the label
        // must not depend on the separator the host happens to recognise.
        for path in [
            "C:\\Program Files\\App\\app.exe",
            "C:/Program Files/App/app.exe",
            "/usr/local/bin/app.exe",
            "app.exe",
            "C:\\Program Files\\App\\app.exe\\",
        ] {
            let launch = Action::Launch {
                program: PathBuf::from(path),
                args: Vec::new(),
            };
            assert_eq!(launch.summary(), "LAUNCH app.exe", "for {path}");
        }
    }

    #[test]
    fn a_tile_never_loses_a_path_that_has_no_segment() {
        let launch = Action::Launch {
            program: PathBuf::from("\\\\"),
            args: Vec::new(),
        };
        assert_eq!(launch.summary(), "LAUNCH \\\\");
    }
}

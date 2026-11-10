//! The actions a key can perform.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ActionError;

/// What a key does when it is pressed.
///
/// The catalogue is deliberately small — a program to run, or a URL to open —
/// because every extra kind is a promise to keep.
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
}

impl Action {
    /// Checks that this action can run.
    ///
    /// ```
    /// use ss550_engine::Action;
    ///
    /// let action = Action::OpenUrl { url: "https://example.com".to_string() };
    /// assert!(action.validate().is_ok());
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
        }
    }

    /// A one-line label for the key tiles: what the key does, briefly.
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
    /// ```
    pub fn summary(&self) -> String {
        match self {
            Action::Launch { program, .. } => format!("LAUNCH {}", file_label(program)),
            Action::OpenUrl { url } => format!("OPEN {}", host_of(url)),
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
fn file_label(program: &Path) -> String {
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

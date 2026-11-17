//! The actions a deck's user reached for last.
//!
//! This list is a **convenience, not a source of truth**: the bindings file
//! says what each key does, and this module only remembers what was used
//! recently so the window can offer it again. Losing it costs a few clicks and
//! nothing else, which is why it is small and bounded, and why a caller may
//! choose to start empty when the file cannot be read.
//!
//! Where the list lives is the caller's decision. The engine is handed a path
//! and never derives one, exactly as it is for bindings and profiles.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::action::Action;

/// The most actions a deck keeps in its recents list.
///
/// Recording one more than this drops the oldest, so the list stays short
/// enough to read at a glance.
pub const MAX_RECENTS: usize = 8;

/// The version of the recents file format this build reads and writes.
const RECENTS_VERSION: u32 = 1;

/// The actions used most recently on one deck, newest first.
///
/// The list holds at most [`MAX_RECENTS`] actions and never holds the same
/// action twice: recording one that is already there moves it to the front.
/// Equality is [`Action`]'s own, so a keystroke recorded as `Ctrl+M` and one
/// read back as `ctrl+m` are the same entry.
///
/// ```
/// use ss550_engine::{Action, Recents};
///
/// let mut recents = Recents::new();
/// let url = Action::OpenUrl {
///     url: "https://twitch.tv/dashboard".to_string(),
/// };
/// let keys = Action::keys("ctrl+shift+m").expect("valid combination");
///
/// recents.record(&url);
/// recents.record(&keys);
///
/// assert_eq!(recents.entries(), &[keys, url]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recents {
    /// Newest first.
    actions: Vec<Action>,
}

impl Recents {
    /// An empty list.
    ///
    /// ```
    /// use ss550_engine::Recents;
    ///
    /// assert!(Recents::new().entries().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// The actions, newest first.
    ///
    /// The slice is never longer than [`MAX_RECENTS`] and never holds the same
    /// action twice.
    pub fn entries(&self) -> &[Action] {
        &self.actions
    }

    /// Puts `action` at the front of the list.
    ///
    /// An action that is already there is **moved**, not duplicated, and the
    /// list keeps at most [`MAX_RECENTS`] entries: recording one more drops the
    /// oldest. The list owns its actions, so recording copies the value; there
    /// is no handle to share, and an [`Action`] is a small value the caller
    /// keeps a copy of anyway.
    pub fn record(&mut self, action: &Action) {
        self.actions.retain(|known| known != action);
        self.actions.insert(0, action.clone());
        self.actions.truncate(MAX_RECENTS);
    }

    /// Reads the list from a file.
    ///
    /// A file that does not exist is an empty list, not a failure: a deck that
    /// has never been used has no recents. Every other failure — an unreadable
    /// file, a corrupt one, a newer format, or an action this build can no
    /// longer validate — is returned as a sentence that says which, so the
    /// caller can decide whether to start empty.
    ///
    /// ```
    /// use ss550_engine::Recents;
    /// use std::path::Path;
    ///
    /// let recents = Recents::load(Path::new("no-such-deck.json")).expect("a missing file is empty");
    /// assert!(recents.entries().is_empty());
    /// ```
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match crate::config::read_json(path) {
            Ok(text) => text,
            // Missing is not broken. Asking for the file and reading it in one
            // step keeps a file deleted between the two from reading as an
            // error.
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(source) => {
                return Err(format!(
                    "could not read the recent actions at {}: {source}",
                    path.display()
                ))
            }
        };
        Self::from_json(&text, path)
    }

    /// Writes the list to a file, creating the folder if it is missing.
    ///
    /// The file carries the format version and the actions newest first, so it
    /// reads back as it was written.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let file = RecentsFileRef {
            version: RECENTS_VERSION,
            actions: &self.actions,
        };
        let text = serde_json::to_string_pretty(&file).map_err(|source| {
            format!(
                "could not write the recent actions to {}: {source}",
                path.display()
            )
        })?;

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|source| format!("could not create {}: {source}", parent.display()))?;
            }
        }

        fs::write(path, text).map_err(|source| {
            format!(
                "could not write the recent actions to {}: {source}",
                path.display()
            )
        })
    }

    /// Reads the list out of JSON that has already been read from `path`.
    ///
    /// `path` is only there to name the file in the error; nothing here touches
    /// the disk.
    fn from_json(text: &str, path: &Path) -> Result<Self, String> {
        let file: RecentsFile = serde_json::from_str(text).map_err(|source| {
            format!(
                "the recent actions file {} is not valid: {source}",
                path.display()
            )
        })?;

        if file.version > RECENTS_VERSION {
            return Err(format!(
                "the recent actions file {} uses format version {}, but this build understands {}",
                path.display(),
                file.version,
                RECENTS_VERSION
            ));
        }

        let mut recents = Self::new();
        // The file is newest first and `record` pushes to the front, so it is
        // replayed oldest first. That keeps the order, keeps the newest copy of
        // an action that appears twice, and applies the same cap as recording.
        for action in file.actions.into_iter().rev() {
            action.validate().map_err(|source| {
                format!(
                    "the recent actions file {} holds an action this build cannot use: {source}",
                    path.display()
                )
            })?;
            recents.record(&action);
        }
        Ok(recents)
    }
}

/// The on-disk shape of the recents file, as it is read.
#[derive(Debug, Deserialize)]
struct RecentsFile {
    version: u32,
    actions: Vec<Action>,
}

/// The same shape, borrowed for writing so saving does not copy the list.
#[derive(Debug, Serialize)]
struct RecentsFileRef<'a> {
    version: u32,
    actions: &'a [Action],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn launch(program: &str) -> Action {
        Action::Launch {
            program: PathBuf::from(program),
            args: Vec::new(),
        }
    }

    fn url(host: &str) -> Action {
        Action::OpenUrl {
            url: format!("https://{host}"),
        }
    }

    fn write(path: &Path, text: &str) {
        fs::write(path, text).expect("write the test's own file");
    }

    fn temp_file(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(name);
        (dir, path)
    }

    #[test]
    fn a_new_list_is_empty() {
        assert!(Recents::new().entries().is_empty());
    }

    #[test]
    fn the_newest_action_is_first() {
        let mut recents = Recents::new();
        let first = launch("a.exe");
        let second = launch("b.exe");

        recents.record(&first);
        recents.record(&second);

        assert_eq!(recents.entries(), &[second, first]);
    }

    #[test]
    fn an_action_already_in_the_list_moves_to_the_front() {
        let mut recents = Recents::new();
        let first = launch("a.exe");
        let second = launch("b.exe");

        recents.record(&first);
        recents.record(&second);
        recents.record(&first);

        assert_eq!(recents.entries(), &[first, second]);
    }

    #[test]
    fn two_spellings_of_one_combination_are_one_entry() {
        // Equality is the action's own, and a combination compares by what it
        // means, not by how it was typed.
        let mut recents = Recents::new();
        let typed = Action::keys("Ctrl+Shift+M").expect("valid combination");
        let stored = Action::keys("ctrl+shift+m").expect("valid combination");

        recents.record(&typed);
        recents.record(&stored);

        assert_eq!(recents.entries().len(), 1);
        assert_eq!(recents.entries(), &[typed]);
    }

    #[test]
    fn the_list_keeps_only_the_newest_eight() {
        let mut recents = Recents::new();
        let all: Vec<Action> = (0..=MAX_RECENTS)
            .map(|index| url(&format!("site{index}.example")))
            .collect();

        for action in &all {
            recents.record(action);
        }

        assert_eq!(recents.entries().len(), MAX_RECENTS);
        assert_eq!(
            recents.entries()[0],
            all[MAX_RECENTS],
            "the newest is on top"
        );
        assert_eq!(
            recents.entries()[MAX_RECENTS - 1],
            all[1],
            "the oldest kept is at the bottom"
        );
        assert!(!recents.entries().contains(&all[0]), "the oldest is gone");
    }

    #[test]
    fn the_file_reads_back_what_it_wrote() {
        let (_dir, path) = temp_file("nested/recents.json");
        let mut recents = Recents::new();
        recents.record(&url("twitch.tv"));
        recents.record(&Action::keys("ctrl+shift+m").expect("valid combination"));
        recents.record(&launch("obs64.exe"));

        recents.save(&path).expect("save");

        assert!(path.exists());
        assert_eq!(Recents::load(&path).expect("load"), recents);
    }

    #[test]
    fn the_file_carries_a_version_and_the_actions_newest_first() {
        let (_dir, path) = temp_file("recents.json");
        let mut recents = Recents::new();
        recents.record(&url("first.example"));
        recents.record(&url("second.example"));
        recents.save(&path).expect("save");

        let text = fs::read_to_string(&path).expect("read");
        let file: serde_json::Value = serde_json::from_str(&text).expect("parse");
        assert_eq!(file["version"], RECENTS_VERSION);
        assert_eq!(file["actions"][0]["url"], "https://second.example");
        assert_eq!(file["actions"][1]["url"], "https://first.example");
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let (_dir, path) = temp_file("never-written.json");

        let recents = Recents::load(&path).expect("a missing file is not a failure");

        assert!(recents.entries().is_empty());
    }

    #[test]
    fn a_corrupt_file_is_reported() {
        let (_dir, path) = temp_file("recents.json");
        write(&path, "{ not json");

        let error = Recents::load(&path).expect_err("a corrupt file must be reported");

        assert!(error.contains("is not valid"), "{error}");
        assert!(error.contains(&path.display().to_string()), "{error}");
    }

    #[test]
    fn a_file_from_the_future_is_refused() {
        let (_dir, path) = temp_file("recents.json");
        write(&path, r#"{"version": 99, "actions": []}"#);

        let error = Recents::load(&path).expect_err("a newer format must be refused");

        assert!(error.contains("format version 99"), "{error}");
        assert!(error.contains(&RECENTS_VERSION.to_string()), "{error}");
    }

    #[test]
    fn an_action_the_build_cannot_use_is_reported() {
        let (_dir, path) = temp_file("recents.json");
        write(
            &path,
            r#"{"version": 1, "actions": [{"type": "open_url", "url": "example.com"}]}"#,
        );

        let error = Recents::load(&path).expect_err("an invalid action must be reported");

        assert!(error.contains("cannot use"), "{error}");
        assert!(error.contains("not a URL"), "{error}");
    }

    #[test]
    fn a_file_with_a_byte_order_mark_still_loads() {
        let (_dir, path) = temp_file("recents.json");
        let mut recents = Recents::new();
        recents.record(&url("twitch.tv"));
        recents.save(&path).expect("save");

        let text = fs::read_to_string(&path).expect("read");
        write(&path, &format!("\u{feff}{text}"));

        assert_eq!(Recents::load(&path).expect("load"), recents);
    }

    #[test]
    fn a_hand_edited_file_is_cut_to_the_same_cap() {
        let (_dir, path) = temp_file("recents.json");
        let actions: Vec<String> = (0..MAX_RECENTS + 2)
            .map(|index| format!(r#"{{"type": "open_url", "url": "https://site{index}.example"}}"#))
            .collect();
        write(
            &path,
            &format!(r#"{{"version": 1, "actions": [{}]}}"#, actions.join(", ")),
        );

        let recents = Recents::load(&path).expect("load");

        assert_eq!(recents.entries().len(), MAX_RECENTS);
        assert_eq!(recents.entries()[0], url("site0.example"));
        assert_eq!(recents.entries()[MAX_RECENTS - 1], url("site7.example"));
    }
}

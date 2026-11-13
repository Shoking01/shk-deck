//! The bindings: what each key does, and where they are kept.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::error::ActionError;
use crate::key::KeyNumber;

/// The version of the bindings file format this build reads and writes.
pub const STORE_VERSION: u32 = 1;

/// What one key does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// The key, numbered the way the UI shows it.
    pub key: KeyNumber,
    /// What pressing it does.
    pub action: Action,
}

/// Every binding the user has set, ordered by key number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionStore {
    bindings: Vec<Binding>,
}

impl ActionStore {
    /// A store with nothing bound yet.
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// How many keys are bound.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Whether no key is bound at all.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// The bindings, in key order.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// What this key does, if anything.
    pub fn get(&self, key: KeyNumber) -> Option<&Action> {
        self.bindings
            .iter()
            .find(|binding| binding.key == key)
            .map(|binding| &binding.action)
    }

    /// Binds a key to an action, replacing any binding it already had.
    ///
    /// The action is checked first: a broken action is never stored. A path
    /// pasted with its wrapping quotes is stored clean, so the bindings file
    /// never keeps the quotes that make `CreateProcess` fail.
    pub fn set(&mut self, key: KeyNumber, action: Action) -> Result<(), ActionError> {
        let action = action.without_wrapping_quotes();
        action.validate()?;
        match self
            .bindings
            .binary_search_by_key(&key, |binding| binding.key)
        {
            Ok(index) => self.bindings[index].action = action,
            Err(index) => self.bindings.insert(index, Binding { key, action }),
        }
        Ok(())
    }

    /// Removes the binding of a key, returning what it did.
    pub fn remove(&mut self, key: KeyNumber) -> Option<Action> {
        let index = self
            .bindings
            .iter()
            .position(|binding| binding.key == key)?;
        Some(self.bindings.remove(index).action)
    }

    /// The store as JSON, ready to be written to disk.
    pub fn to_json(&self) -> Result<String, ActionError> {
        let file = StoreFile {
            version: STORE_VERSION,
            bindings: self.bindings.clone(),
        };
        serde_json::to_string_pretty(&file).map_err(|source| ActionError::Serialise { source })
    }

    /// Reads a store from JSON.
    pub fn from_json(text: &str) -> Result<Self, ActionError> {
        let file: StoreFile =
            serde_json::from_str(text).map_err(|source| ActionError::InvalidJson { source })?;
        if file.version > STORE_VERSION {
            return Err(ActionError::UnsupportedVersion {
                found: file.version,
                expected: STORE_VERSION,
            });
        }
        let mut store = Self::new();
        for binding in file.bindings {
            store.set(binding.key, binding.action)?;
        }
        Ok(store)
    }

    /// Loads the store from a file.
    ///
    /// A missing file is not a failure: it means the user has not bound
    /// anything yet.
    pub fn load(path: &Path) -> Result<Self, ActionError> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let text = fs::read_to_string(path).map_err(|source| ActionError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&text)
    }

    /// Writes the store to a file, creating the folder if it is missing.
    pub fn save(&self, path: &Path) -> Result<(), ActionError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|source| ActionError::Write {
                    path: parent.display().to_string(),
                    source,
                })?;
            }
        }
        let text = self.to_json()?;
        fs::write(path, text).map_err(|source| ActionError::Write {
            path: path.display().to_string(),
            source,
        })
    }
}

/// The on-disk shape of the bindings file.
#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    bindings: Vec<Binding>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn key(number: u8) -> KeyNumber {
        KeyNumber::new(number).expect("valid key number")
    }

    fn launch() -> Action {
        Action::Launch {
            program: PathBuf::from("C:\\Program Files\\Obsidian\\Obsidian.exe"),
            args: vec!["--profile".to_string(), "Live".to_string()],
        }
    }

    fn open_url() -> Action {
        Action::OpenUrl {
            url: "https://twitch.tv/dashboard".to_string(),
        }
    }

    #[test]
    fn a_new_store_has_no_bindings() {
        let store = ActionStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn a_bound_key_keeps_its_action() {
        let mut store = ActionStore::new();
        store.set(key(3), launch()).expect("valid action");
        assert_eq!(store.get(key(3)), Some(&launch()));
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());
    }

    #[test]
    fn binding_a_key_twice_replaces_the_action() {
        let mut store = ActionStore::new();
        store.set(key(3), launch()).expect("valid action");
        store.set(key(3), open_url()).expect("valid action");
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(key(3)), Some(&open_url()));
    }

    #[test]
    fn clearing_a_key_removes_only_that_key() {
        let mut store = ActionStore::new();
        store.set(key(3), launch()).expect("valid action");
        store.set(key(4), open_url()).expect("valid action");
        assert!(store.remove(key(3)).is_some());
        assert!(store.get(key(3)).is_none());
        assert!(store.get(key(4)).is_some());
    }

    #[test]
    fn a_pasted_path_is_saved_without_its_quotes() {
        let mut store = ActionStore::new();
        store
            .set(
                key(1),
                Action::Launch {
                    program: "\"C:\\Users\\me\\App\\app.exe\"".into(),
                    args: Vec::new(),
                },
            )
            .expect("valid action");

        let Action::Launch { program, .. } = store.get(key(1)).expect("bound") else {
            panic!("still a launch");
        };
        assert_eq!(program.to_str(), Some("C:\\Users\\me\\App\\app.exe"));

        let json = store.to_json().expect("serialise");
        assert!(
            !json.contains("\\\""),
            "the file must not keep the quotes: {json}"
        );

        // And a file that still has them is cleaned on load.
        let legacy = r#"{"version":1,"bindings":[{"key":2,"action":{"type":"launch","program":"\"C:\\x\\y.exe\"","args":[]}}]}"#;
        let restored = ActionStore::from_json(legacy).expect("parse");
        let Action::Launch { program, .. } = restored.get(key(2)).expect("bound") else {
            panic!("still a launch");
        };
        assert_eq!(program.to_str(), Some("C:\\x\\y.exe"));
    }

    #[test]
    fn an_invalid_action_is_refused() {
        let mut store = ActionStore::new();
        let broken = Action::OpenUrl {
            url: "not a url".to_string(),
        };
        assert!(store.set(key(1), broken).is_err());
        assert!(store.is_empty());
    }

    #[test]
    fn a_store_round_trips_through_json() {
        let mut store = ActionStore::new();
        store.set(key(1), launch()).expect("valid action");
        store.set(key(15), open_url()).expect("valid action");
        let json = store.to_json().expect("serialise");
        let restored = ActionStore::from_json(&json).expect("parse");
        assert_eq!(restored.get(key(1)), Some(&launch()));
        assert_eq!(restored.get(key(15)), Some(&open_url()));
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn the_json_holds_the_version_and_the_keys() {
        let mut store = ActionStore::new();
        store.set(key(7), open_url()).expect("valid action");
        let json = store.to_json().expect("serialise");
        assert!(json.contains(&format!("\"version\": {STORE_VERSION}")));
        assert!(json.contains("\"key\": 7"));
        assert!(json.contains("\"type\": \"open_url\""));
    }

    #[test]
    fn a_missing_file_means_no_bindings() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("bindings.json");
        let store = ActionStore::load(&path).expect("a missing file is not an error");
        assert!(store.is_empty());
    }

    #[test]
    fn saving_creates_the_folder_and_the_file_reads_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("bindings.json");
        let mut store = ActionStore::new();
        store.set(key(2), open_url()).expect("valid action");
        store.save(&path).expect("save");
        assert!(path.exists());
        let reloaded = ActionStore::load(&path).expect("load");
        assert_eq!(reloaded.get(key(2)), Some(&open_url()));
    }

    #[test]
    fn a_corrupt_file_is_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("bindings.json");
        fs::write(&path, "{not json").expect("write");
        let error = ActionStore::load(&path).expect_err("must fail");
        assert!(matches!(error, ActionError::InvalidJson { .. }));
    }

    #[test]
    fn a_file_from_the_future_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("bindings.json");
        fs::write(&path, r#"{"version": 99, "bindings": []}"#).expect("write");
        let error = ActionStore::load(&path).expect_err("must fail");
        assert!(matches!(
            error,
            ActionError::UnsupportedVersion {
                found: 99,
                expected: STORE_VERSION
            }
        ));
    }
}

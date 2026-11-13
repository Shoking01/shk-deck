//! Named sets of bindings and key colors.
//!
//! A profile is a named collection of bindings (what each key does) and
//! optionally key colors (what each key displays). Profiles are persisted as
//! JSON in `config_dir()/profiles/<slug>-<fnv64>.json`.
//!
//! Six default profiles are provided:
//! - "default": empty bindings, all keys white (blank)
//! - "stream": Twitch/YouTube stream key bindings
//! - "meetings": mute/unmute, camera on/off, leave meeting, etc.
//! - "gaming": quick actions for popular games
//! - "work": browser, IDE, email, calendar
//! - "entertainment": media controls, Netflix, Spotify, games

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ActionError;
use crate::store::ActionStore;

/// A profile with its name, bindings and key colors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// The profile name, shown in the UI.
    pub name: String,
    /// What each key does.
    pub bindings: ActionStore,
    /// What color each key shows, as RGB (0-255). Not all keys need to be listed.
    #[serde(default)]
    pub colors: BTreeMap<u16, [u8; 3]>,
    /// The slot (color theme) this profile uses when not painting a specific color.
    /// "white", "red", or "blank" (which is white).
    #[serde(skip_serializing_if = "String::is_empty", default = "default_slot")]
    pub slot: String,
}

impl Profile {
    /// Creates a new empty profile with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            bindings: ActionStore::new(),
            colors: BTreeMap::new(),
            slot: String::new(),
        }
    }

    /// Returns the color for a key, or None if using the slot color.
    pub fn key_color(&self, key_number: u16) -> Option<[u8; 3]> {
        self.colors.get(&key_number).copied()
    }

    /// Returns the slot name (white, red, etc.)
    pub fn slot_name(&self) -> &str {
        &self.slot
    }
}

fn default_slot() -> String {
    "white".to_string()
}

/// The profiles owned by a deck.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileStore {
    profiles: BTreeMap<String, Profile>,
    active: Option<String>,
}

impl ProfileStore {
    /// Creates a store with the default empty profile.
    pub fn new() -> Self {
        let defaults = create_default_profiles();
        let mut profiles = BTreeMap::new();
        for (name, profile) in defaults {
            profiles.insert(name, profile);
        }
        Self {
            profiles,
            active: Some("default".to_string()),
        }
    }

    /// Returns all profile names, in sorted order.
    pub fn names(&self) -> Vec<&str> {
        self.profiles.keys().map(|s| s.as_str()).collect()
    }

    /// Returns all profile names as owned strings, in sorted order.
    pub fn profile_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.profiles.keys().cloned().collect();
        names.sort();
        names
    }

    /// Returns the active profile, if any.
    pub fn active(&self) -> Option<&Profile> {
        self.active
            .as_ref()
            .and_then(|name| self.profiles.get(name))
    }

    /// Returns the active profile name.
    pub fn active_name(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Returns all profiles as a vector of references.
    pub fn profiles(&self) -> Vec<&Profile> {
        let mut profiles: Vec<&Profile> = self.profiles.values().collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        profiles
    }

    /// Switches to named profile.
    pub fn activate(&mut self, name: &str) -> Result<(), String> {
        if !self.profiles.contains_key(name) {
            return Err(format!("no such profile: {name}"));
        }
        self.active = Some(name.to_string());
        Ok(())
    }

    /// Updates the active profile's bindings and saves to disk.
    pub fn update_bindings(
        &mut self,
        bindings: &ActionStore,
        deck_key: &str,
        config_dir: &Path,
    ) -> Result<(), String> {
        let active_name = self.active.clone();
        if let Some(name) = active_name {
            if let Some(profile) = self.profiles.get_mut(&name) {
                profile.bindings = bindings.clone();
                return self
                    ._save_to(&config_dir.join("profiles").join(format!("{deck_key}.json")));
            }
        }
        Ok(())
    }

    /// Returns the bindings for the active profile.
    pub fn active_bindings(&self) -> Option<ActionStore> {
        self.active().map(|p| p.bindings.clone())
    }

    /// Merges bindings from the active profile into `target`.
    pub fn apply_bindings(&self, target: &mut ActionStore) -> Result<(), ActionError> {
        if let Some(active) = self.active() {
            *target = active.bindings.clone();
            Ok(())
        } else {
            Err(ActionError::ProfileError("no active profile".to_string()))
        }
    }

    /// Returns a copy of the active profile with its bindings and colors.
    pub fn active_profile(&self) -> Option<Profile> {
        self.active().cloned()
    }

    /// Paints all keys according to the active profile.
    /// Returns (bindings, key_colors, slot) to apply to the deck.
    pub fn active_profile_colors(&self) -> Option<(ActionStore, BTreeMap<u16, [u8; 3]>, String)> {
        let active = self.active()?;
        Some((
            active.bindings.clone(),
            active.colors.clone(),
            active.slot.clone(),
        ))
    }

    /// Saves this store to disk, keyed by the deck's slug.
    pub fn save(&self, deck_key: &str, config_dir: &Path) -> Result<(), String> {
        let path = config_dir.join("profiles").join(format!("{deck_key}.json"));
        self._save_to(&path)
    }

    fn _save_to(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        path.parent()
            .map(std::fs::create_dir_all)
            .transpose()
            .map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Loads from disk, keyed by the deck's slug.
    pub fn load(deck_key: &str, config_dir: &Path) -> Result<Self, String> {
        let path = config_dir.join("profiles").join(format!("{deck_key}.json"));
        Self::_load_from(&path)
    }

    fn _load_from(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }
}

/// Creates six profile templates.
pub fn create_default_profiles() -> BTreeMap<String, Profile> {
    let mut map = BTreeMap::new();

    // default: blank slate
    map.insert(
        "default".to_string(),
        Profile {
            name: "default".to_string(),
            bindings: ActionStore::new(),
            colors: BTreeMap::new(),
            slot: "white".to_string(),
        },
    );

    // stream: common streaming actions
    let stream = ActionStore::new();
    map.insert(
        "stream".to_string(),
        Profile {
            name: "stream".to_string(),
            bindings: stream,
            colors: BTreeMap::from([
                (1, [255, 0, 0]),   // Start/Stop Stream (red)
                (2, [0, 255, 0]),   // Mute (green)
                (3, [0, 0, 255]),   // Camera (blue)
                (4, [255, 255, 0]), // Stop Recording (yellow)
                (5, [255, 0, 255]), // Switch Scene (magenta)
            ]),
            slot: "red".to_string(),
        },
    );

    // meetings: zoom/teamspeak/etc
    let meetings = ActionStore::new();
    map.insert(
        "meetings".to_string(),
        Profile {
            name: "meetings".to_string(),
            bindings: meetings,
            colors: BTreeMap::from([
                (1, [255, 0, 0]),   // Leave Meeting (red)
                (2, [0, 255, 0]),   // Mute (green)
                (3, [0, 0, 255]),   // Webcam (blue)
                (4, [255, 255, 0]), // Share Screen (yellow)
            ]),
            slot: "white".to_string(),
        },
    );

    // gaming: switch game profile
    let gaming = ActionStore::new();
    map.insert(
        "gaming".to_string(),
        Profile {
            name: "gaming".to_string(),
            bindings: gaming,
            colors: BTreeMap::from([(1, [200, 0, 0]), (2, [0, 200, 0]), (3, [0, 0, 200])]),
            slot: "red".to_string(),
        },
    );

    // work: productivity tools
    let work = ActionStore::new();
    map.insert(
        "work".to_string(),
        Profile {
            name: "work".to_string(),
            bindings: work,
            colors: BTreeMap::new(),
            slot: "white".to_string(),
        },
    );

    // entertainment: media controls
    let entertainment = ActionStore::new();
    map.insert(
        "entertainment".to_string(),
        Profile {
            name: "entertainment".to_string(),
            bindings: entertainment,
            colors: BTreeMap::from([
                (1, [200, 0, 0]),   // Play/Pause
                (2, [0, 200, 0]),   // Next
                (3, [0, 0, 200]),   // Previous
                (4, [200, 200, 0]), // Stop
                (5, [200, 0, 200]), // Mute
            ]),
            slot: "white".to_string(),
        },
    );

    map
}

/// A RGB colour as an integer 0xRRGGBB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(u8, u8, u8);

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b)
    }

    pub fn to_rgb(&self) -> [u8; 3] {
        [self.0, self.1, self.2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_store_has_six_profiles() {
        let store = ProfileStore::new();
        assert_eq!(store.names().len(), 6);
        assert!(store.names().contains(&"default"));
        assert!(store.names().contains(&"stream"));
        assert!(store.names().contains(&"meetings"));
    }

    #[test]
    fn the_active_profile_is_default() {
        let store = ProfileStore::new();
        assert_eq!(store.active_name(), Some("default"));
    }

    #[test]
    fn activating_an_unknown_profile_fails() {
        let mut store = ProfileStore::new();
        assert!(store.activate("nonexistent").is_err());
    }

    #[test]
    fn profiles_round_trip_through_json() {
        let store = ProfileStore::new();
        let json = serde_json::to_string(&store).expect("serialize");
        let restored: ProfileStore = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.names(), store.names());
    }
}

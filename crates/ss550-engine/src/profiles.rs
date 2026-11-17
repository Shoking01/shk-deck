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

use crate::appearance::Appearance;
use crate::error::{ActionError, TitleError};
use crate::store::ActionStore;

/// The longest title a key accepts, in characters.
///
/// A key is about 108 pixels on a side and the window draws a title with a 5x7
/// bitmap font at roughly six to eight characters per line. Thirty characters
/// is at most five short lines: it fits the tile at that size without shrinking
/// the font, and it stays readable at a glance. The cap counts characters, not
/// bytes, so an accented title is not penalised for its encoding.
pub const MAX_TITLE_CHARS: usize = 30;

/// A profile with its name, bindings and what each key shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// The profile name, shown in the UI.
    pub name: String,
    /// What each key does.
    pub bindings: ActionStore,
    /// What each key shows, by the key number the UI uses. A key that is not
    /// listed falls back to [`Profile::slot`]. This is the only source of
    /// truth for a key's appearance.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub appearance: BTreeMap<u16, Appearance>,
    /// The short text each key shows, by the key number the UI uses.
    ///
    /// Written only through [`Profile::set_title`], which trims and bounds the
    /// text, so every value here is already valid. The map is skipped when it
    /// is empty and defaults to empty when it is absent, so a profile file
    /// written before titles existed loads unchanged.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    titles: BTreeMap<u16, String>,
    /// The flat colour a key shows when [`Profile::appearance`] has no entry
    /// for it.
    ///
    /// Legacy on purpose: profiles written before M6 stored their colours here,
    /// as `key -> rgb`. It is read and migrated into [`Profile::appearance`] on
    /// load, and written empty from then on, so the two can never disagree.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<u16, [u8; 3]>,
    /// The slot (colour theme) this profile uses when not painting a specific colour.
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
            appearance: BTreeMap::new(),
            titles: BTreeMap::new(),
            colors: BTreeMap::new(),
            // A profile with nothing on it is blank, not white: a key the user
            // has not touched should look like a key with nothing on it.
            slot: "blank".to_string(),
        }
    }

    /// Creates a profile whose keys show these flat colours.
    pub fn with_solids(name: &str, slot: &str, solids: &[(u16, [u8; 3])]) -> Self {
        Self {
            name: name.to_string(),
            bindings: ActionStore::new(),
            appearance: solids
                .iter()
                .map(|(key, rgb)| (*key, Appearance::solid(*rgb)))
                .collect(),
            titles: BTreeMap::new(),
            colors: BTreeMap::new(),
            slot: slot.to_string(),
        }
    }

    /// The short text this key shows, when the profile gives it one.
    pub fn title(&self, key: u16) -> Option<&str> {
        self.titles.get(&key).map(String::as_str)
    }

    /// Sets the short text this key shows, or removes it.
    ///
    /// The title is trimmed before it is stored. An empty or whitespace-only
    /// title removes the entry instead of storing an empty string, so "no
    /// title" has exactly one representation. A title longer than
    /// [`MAX_TITLE_CHARS`] characters, or holding a control character, is
    /// refused with a [`TitleError`] and leaves the key as it was.
    ///
    /// Titles are independent of [`Profile::appearance`] and of the bindings:
    /// setting one changes neither.
    ///
    /// # Errors
    ///
    /// Returns [`TitleError::TooLong`] when the trimmed title is longer than
    /// [`MAX_TITLE_CHARS`], and [`TitleError::ControlCharacter`] when it holds
    /// a character the drawing cannot show.
    pub fn set_title(&mut self, key: u16, title: Option<&str>) -> Result<(), TitleError> {
        let Some(title) = title else {
            self.titles.remove(&key);
            return Ok(());
        };
        let title = title.trim();
        if title.is_empty() {
            self.titles.remove(&key);
            return Ok(());
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(TitleError::TooLong {
                max: MAX_TITLE_CHARS,
            });
        }
        if title.chars().any(char::is_control) {
            return Err(TitleError::ControlCharacter);
        }
        self.titles.insert(key, title.to_string());
        Ok(())
    }

    /// The colour for a key that shows one, or `None` when it shows an image or
    /// nothing of its own.
    pub fn key_color(&self, key_number: u16) -> Option<[u8; 3]> {
        self.appearance.get(&key_number).and_then(Appearance::rgb)
    }

    /// What this key shows, when the profile says.
    pub fn appearance_for(&self, key_number: u16) -> Option<&Appearance> {
        self.appearance.get(&key_number)
    }

    /// Moves a legacy `colors` entry into `appearance`, keeping anything the
    /// appearance already says about that key.
    fn migrate_legacy_colors(&mut self) {
        for (key, rgb) in std::mem::take(&mut self.colors) {
            self.appearance
                .entry(key)
                .or_insert(Appearance::Solid { rgb });
        }
    }

    /// Returns the slot name (white, red, etc.)
    pub fn slot_name(&self) -> &str {
        &self.slot
    }
}

fn default_slot() -> String {
    "blank".to_string()
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
                return self._save_to(&crate::config::profiles_path_in(config_dir, deck_key));
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

    /// The flat colours the active profile paints, for the keys that show one.
    ///
    /// A key showing an image is absent here on purpose: a caller that only
    /// knows about colours cannot paint over an image by accident.
    pub fn active_profile_colors(&self) -> Option<(ActionStore, BTreeMap<u16, [u8; 3]>, String)> {
        let active = self.active()?;
        let solids = active
            .appearance
            .iter()
            .filter_map(|(key, appearance)| appearance.rgb().map(|rgb| (*key, rgb)))
            .collect();
        Some((active.bindings.clone(), solids, active.slot.clone()))
    }

    /// The appearance of every key the active profile says something about.
    pub fn active_profile_appearance(&self) -> Option<BTreeMap<u16, Appearance>> {
        Some(self.active()?.appearance.clone())
    }

    /// Replaces what the active profile says one key shows, or removes it.
    ///
    /// `None` makes the key fall back to the profile's slot colour, which is
    /// what "no artwork" means. A store with no active profile is an error
    /// rather than a silent no-op: a caller that just assigned a picture needs
    /// to know the file it believes it wrote was not written.
    /// Sets or removes the title of a key on the active profile.
    ///
    /// `None` (or a title that trims to nothing) removes it. The validation is
    /// [`Profile::set_title`]'s, so the store cannot accept a title the profile
    /// would refuse.
    ///
    /// # Errors
    ///
    /// The profile's own [`TitleError`], or a message naming the missing active
    /// profile.
    pub fn set_title(&mut self, key: u16, title: Option<&str>) -> Result<(), String> {
        let name = self
            .active
            .clone()
            .ok_or_else(|| "no active profile".to_string())?;
        let profile = self
            .profiles
            .get_mut(&name)
            .ok_or_else(|| format!("no such profile: {name}"))?;
        profile
            .set_title(key, title)
            .map_err(|error| error.to_string())
    }

    pub fn set_appearance(
        &mut self,
        key: u16,
        appearance: Option<Appearance>,
    ) -> Result<(), String> {
        let name = self
            .active
            .clone()
            .ok_or_else(|| "no active profile".to_string())?;
        let profile = self
            .profiles
            .get_mut(&name)
            .ok_or_else(|| format!("no such profile: {name}"))?;
        match appearance {
            Some(appearance) => {
                profile.appearance.insert(key, appearance);
            }
            None => {
                profile.appearance.remove(&key);
            }
        }
        Ok(())
    }

    /// Saves this store to disk, keyed by the deck's slug.
    pub fn save(&self, deck_key: &str, config_dir: &Path) -> Result<(), String> {
        let path = crate::config::profiles_path_in(config_dir, deck_key);
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
        let path = crate::config::profiles_path_in(config_dir, deck_key);
        Self::_load_from(&path)
    }

    fn _load_from(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let text = crate::config::read_json(path).map_err(|e| e.to_string())?;
        let mut store: Self = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        for profile in store.profiles.values_mut() {
            profile.migrate_legacy_colors();
        }
        Ok(store)
    }
}

/// Creates six profile templates.
pub fn create_default_profiles() -> BTreeMap<String, Profile> {
    let mut map = BTreeMap::new();
    let mut add = |profile: Profile| {
        map.insert(profile.name.clone(), profile);
    };

    // Six profiles, all blank. The names are the feature — a deck can keep one
    // set of bindings per situation — and the colours that used to come with
    // them were a test fixture: a key with nothing on it should look like
    // nothing, not like a red button nobody chose.
    for name in [
        "default",
        "stream",
        "meetings",
        "gaming",
        "work",
        "entertainment",
    ] {
        add(Profile::new(name));
    }

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
    use crate::assets::AssetName;

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

    #[test]
    fn the_default_profiles_carry_no_colours() {
        let store = ProfileStore::new();
        for profile in store.profiles() {
            assert!(
                profile.appearance.is_empty(),
                "{} must start blank, not painted",
                profile.name
            );
            assert_eq!(profile.slot_name(), "blank", "{}", profile.name);
        }
    }

    #[test]
    fn a_profile_written_before_the_appearance_field_still_loads_its_colours() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("profiles").join("deck.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create");
        std::fs::write(
            &path,
            r#"{
                "profiles": {
                    "stream": {
                        "name": "stream",
                        "bindings": { "version": 1, "bindings": [] },
                        "colors": { "1": [255, 0, 0], "4": [255, 255, 0] },
                        "slot": "red"
                    }
                },
                "active": "stream"
            }"#,
        )
        .expect("write the legacy file");

        let store = ProfileStore::_load_from(&path).expect("load");
        let stream = store.active().expect("the active profile");

        assert_eq!(
            stream.appearance_for(1),
            Some(&Appearance::solid([255, 0, 0]))
        );
        assert_eq!(
            stream.appearance_for(4),
            Some(&Appearance::solid([255, 255, 0]))
        );
        assert!(
            stream.colors.is_empty(),
            "the legacy map is drained on load, not kept alongside"
        );
    }

    #[test]
    fn an_appearance_the_legacy_field_also_mentions_keeps_the_appearance() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("profiles").join("deck.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create");
        std::fs::write(
            &path,
            r#"{
                "profiles": {
                    "stream": {
                        "name": "stream",
                        "bindings": { "version": 1, "bindings": [] },
                        "appearance": { "1": { "kind": "image", "asset": "live.png" } },
                        "colors": { "1": [255, 0, 0] },
                        "slot": "red"
                    }
                },
                "active": "stream"
            }"#,
        )
        .expect("write the mixed file");

        let store = ProfileStore::_load_from(&path).expect("load");
        let stream = store.active().expect("the active profile");

        assert_eq!(
            stream
                .appearance_for(1)
                .and_then(Appearance::asset)
                .map(AssetName::as_str),
            Some("live.png"),
            "an explicit appearance outranks the legacy colour"
        );
        assert_eq!(stream.key_color(1), None);
    }

    #[test]
    fn an_image_appearance_survives_a_save_and_a_load() {
        let dir = tempfile::tempdir().expect("temp dir");
        let name = AssetName::parse("live.png").expect("valid name");

        let mut store = ProfileStore::new();
        store.activate("stream").expect("activate");
        store
            .profiles
            .get_mut("stream")
            .expect("the stream profile")
            .appearance
            .insert(2, Appearance::image(name.clone()));
        store.save("deck", dir.path()).expect("save");

        let loaded = ProfileStore::load("deck", dir.path()).expect("load");
        let stream = loaded.active().expect("the active profile");

        assert_eq!(stream.appearance_for(2), Some(&Appearance::image(name)));
        assert_eq!(stream.key_color(2), None, "an image is not a colour");
    }

    #[test]
    fn the_active_colours_leave_out_the_keys_that_show_an_image() {
        let name = AssetName::parse("live.png").expect("valid name");

        let mut store = ProfileStore::new();
        store.activate("stream").expect("activate");
        let stream = store
            .profiles
            .get_mut("stream")
            .expect("the stream profile");
        stream.appearance.insert(1, Appearance::solid([255, 0, 0]));
        stream
            .appearance
            .insert(5, Appearance::solid([255, 0, 255]));
        stream.appearance.insert(3, Appearance::image(name));

        let (_, solids, _) = store.active_profile_colors().expect("the active profile");
        assert_eq!(solids.get(&1), Some(&[255, 0, 0]));
        assert_eq!(solids.get(&5), Some(&[255, 0, 255]));
        assert_eq!(
            solids.get(&3),
            None,
            "a key showing an image must not be paintable as a colour"
        );
    }

    #[test]
    fn setting_an_appearance_replaces_what_the_key_shows_and_can_take_it_back() {
        let name = AssetName::parse("live.png").expect("valid name");

        let mut store = ProfileStore::new();
        store.activate("stream").expect("activate");

        store
            .set_appearance(1, Some(Appearance::image(name.clone())))
            .expect("set the picture");
        let stream = store.active().expect("the active profile");
        assert_eq!(stream.appearance_for(1), Some(&Appearance::image(name)));
        assert_eq!(
            stream.key_color(1),
            None,
            "a picture is not a colour to paint with"
        );

        store.set_appearance(1, None).expect("remove the picture");
        assert_eq!(
            store
                .active()
                .expect("the active profile")
                .appearance_for(1),
            None,
            "removing the artwork leaves the key with no appearance of its own"
        );
    }

    #[test]
    fn a_title_can_be_set_read_and_removed() {
        let mut profile = Profile::new("stream");
        assert_eq!(profile.title(1), None, "a new profile titles nothing");

        profile.set_title(1, Some("Mute")).expect("a short title");
        assert_eq!(profile.title(1), Some("Mute"));
        assert_eq!(
            profile.appearance_for(1),
            None,
            "a title is not an appearance"
        );

        profile.set_title(1, None).expect("remove the title");
        assert_eq!(profile.title(1), None);
        assert!(
            profile.titles.is_empty(),
            "removing the only title leaves the map empty, not holding a hole"
        );
    }

    #[test]
    fn a_title_is_stored_trimmed() {
        let mut profile = Profile::new("stream");
        profile
            .set_title(2, Some("  Live  "))
            .expect("a padded title");
        assert_eq!(profile.title(2), Some("Live"));
    }

    #[test]
    fn a_whitespace_only_title_removes_the_entry() {
        let mut profile = Profile::new("stream");
        profile.set_title(3, Some("Chat")).expect("set a title");
        profile
            .set_title(3, Some("   \t "))
            .expect("whitespace is not a title");
        assert_eq!(profile.title(3), None);
        assert!(profile.titles.is_empty());
    }

    #[test]
    fn a_title_longer_than_the_cap_is_refused() {
        let mut profile = Profile::new("stream");
        let too_long = "a".repeat(MAX_TITLE_CHARS + 1);

        let error = profile
            .set_title(1, Some(&too_long))
            .expect_err("over the cap");

        assert_eq!(
            error,
            TitleError::TooLong {
                max: MAX_TITLE_CHARS
            }
        );
        assert_eq!(error.to_string(), "a title can hold at most 30 characters");
        assert_eq!(
            profile.title(1),
            None,
            "a refused title leaves the key as it was"
        );
    }

    #[test]
    fn a_title_exactly_at_the_cap_is_kept() {
        let mut profile = Profile::new("stream");
        let longest = "a".repeat(MAX_TITLE_CHARS);
        profile
            .set_title(1, Some(&longest))
            .expect("exactly the cap fits");
        assert_eq!(profile.title(1), Some(longest.as_str()));
    }

    #[test]
    fn a_title_with_a_control_character_is_refused() {
        let mut profile = Profile::new("stream");

        let error = profile
            .set_title(1, Some("live\nnow"))
            .expect_err("a newline is not printable text");

        assert_eq!(error, TitleError::ControlCharacter);
        assert_eq!(
            error.to_string(),
            "a title cannot contain control characters"
        );
        assert_eq!(profile.title(1), None);
    }

    #[test]
    fn a_profile_written_before_titles_existed_loads_without_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("profiles").join("deck.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create");
        std::fs::write(
            &path,
            r#"{
                "profiles": {
                    "stream": {
                        "name": "stream",
                        "bindings": { "version": 1, "bindings": [] },
                        "appearance": { "1": { "kind": "solid", "rgb": [255, 0, 0] } },
                        "slot": "red"
                    }
                },
                "active": "stream"
            }"#,
        )
        .expect("write the old file, which has no titles key");

        let store = ProfileStore::_load_from(&path).expect("load");
        let stream = store.active().expect("the active profile");

        assert_eq!(
            stream.title(1),
            None,
            "an absent titles map means no key has a title"
        );
        assert!(stream.titles.is_empty());
        assert_eq!(
            stream.appearance_for(1),
            Some(&Appearance::solid([255, 0, 0])),
            "the rest of an old profile still loads"
        );
    }

    #[test]
    fn titles_survive_a_save_and_a_load() {
        let dir = tempfile::tempdir().expect("temp dir");

        let mut store = ProfileStore::new();
        store.activate("stream").expect("activate");
        {
            let profile = store
                .profiles
                .get_mut("stream")
                .expect("the stream profile");
            profile.set_title(1, Some("Mute")).expect("set");
            profile.set_title(7, Some("Cam")).expect("set");
        }
        store.save("deck", dir.path()).expect("save");

        let loaded = ProfileStore::load("deck", dir.path()).expect("load");
        let stream = loaded.active().expect("the active profile");

        assert_eq!(stream.title(1), Some("Mute"));
        assert_eq!(stream.title(7), Some("Cam"));
        assert_eq!(
            stream.title(2),
            None,
            "a key without a title stays untitled"
        );
    }

    #[test]
    fn a_profile_with_no_titles_writes_no_titles_key() {
        let store = ProfileStore::new();
        let json = serde_json::to_string(&store).expect("serialize");
        assert!(
            !json.contains("\"titles\""),
            "an empty titles map must not be written: {json}"
        );
    }

    #[test]
    fn a_title_is_written_under_its_key_number() {
        let mut profile = Profile::new("stream");
        profile.set_title(3, Some("Chat")).expect("set");
        let json = serde_json::to_string(&profile).expect("serialize");
        assert!(
            json.contains(r#""titles":{"3":"Chat"}"#),
            "titles are a key-to-text object: {json}"
        );
    }
}

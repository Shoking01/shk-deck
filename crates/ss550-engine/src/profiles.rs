//! Named sets of pages, bindings and key colours.
//!
//! A profile is a named collection of **pages**. The profile's own keys are its
//! root page — the one it shows when it is activated — and under it live any
//! number of named pages, which is what a folder is on the deck: a key that swaps
//! the whole set for another and a key that swaps it back.
//!
//! ## The file
//!
//! The root page is written **flat**: `bindings`, `appearance`, `titles`,
//! `title_styles` and `slot` sit at the profile's own level, exactly as they did
//! before pages existed, and a `pages` object appears only once a folder is
//! created. So a profile file written by any earlier build loads here unchanged
//! and means the same thing, with no migration step and no version bump.
//!
//! ```json
//! {
//!   "name": "default",
//!   "bindings": { "bindings": [ { "key": 4, "action": { "type": "folder", "page": "Media" } } ] },
//!   "pages": {
//!     "Media": {
//!       "bindings": { "bindings": [ { "key": 4, "action": { "type": "back" } } ] }
//!     }
//!   },
//!   "slot": "blank"
//! }
//! ```
//!
//! Profiles are persisted as JSON in
//! `config_dir()/profiles/<slug>-<fnv64>.json`.
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
use crate::key::KeyNumber;
use crate::store::ActionStore;
use crate::title::TitleStyle;

/// The longest title a key accepts, in characters.
///
/// A key is about 108 pixels on a side and the window draws a title with a 5x7
/// bitmap font at roughly six to eight characters per line. Thirty characters
/// is at most five short lines: it fits the tile at that size without shrinking
/// the font, and it stays readable at a glance. The cap counts characters, not
/// bytes, so an accented title is not penalised for its encoding.
pub const MAX_TITLE_CHARS: usize = 30;

/// What each key of one page does and shows.
///
/// A page is one screen of keys. A profile has a **root page** — the keys the
/// profile has always had — and any number of named pages under it, which is
/// what a folder is on the deck: one key swaps the whole set for another and
/// another key swaps it back.
///
/// Everything here is per page on purpose. Two pages of one profile may bind the
/// same physical key to different things, give it a different colour, a
/// different picture and a different title; that is the whole point of having
/// them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageKeys {
    /// What each key does.
    pub bindings: ActionStore,
    /// What each key shows, by the key number the UI uses. A key that is not
    /// listed falls back to [`PageKeys::slot`]. This is the only source of
    /// truth for a key's appearance on this page.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub appearance: BTreeMap<u16, Appearance>,
    /// The short text each key shows, by the key number the UI uses.
    ///
    /// Written only through [`PageKeys::set_title`], which trims and bounds the
    /// text, so every value here is already valid. The map is skipped when it
    /// is empty and defaults to empty when it is absent, so a page written
    /// before titles existed loads unchanged.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    titles: BTreeMap<u16, String>,
    /// How a key's title is drawn, when it differs from the app's default.
    ///
    /// Kept beside the words rather than in a second source of truth: a key with
    /// no entry here is drawn with the default the window keeps, and a key with
    /// no title is absent from both maps. Written only through
    /// [`PageKeys::set_title_style`], which clamps the size, and clamped again on
    /// load, so every value in use is one a tile can hold.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    title_styles: BTreeMap<u16, TitleStyle>,
    /// The flat colour a key shows when [`PageKeys::appearance`] has no entry
    /// for it.
    ///
    /// Legacy on purpose: profiles written before M6 stored their colours here,
    /// as `key -> rgb`. It is read and migrated into [`PageKeys::appearance`] on
    /// load, and written empty from then on, so the two can never disagree.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<u16, [u8; 3]>,
    /// The slot (colour theme) this page uses when not painting a specific
    /// colour. "white", "red", or "blank".
    #[serde(skip_serializing_if = "String::is_empty", default = "default_slot")]
    pub slot: String,
}

impl PageKeys {
    /// A page with the given theme and no keys bound.
    fn with_slot(slot: &str) -> Self {
        Self {
            slot: slot.to_string(),
            ..Self::default()
        }
    }

    /// The short text this key shows, when the page gives it one.
    pub fn title(&self, key: u16) -> Option<&str> {
        self.titles.get(&key).map(String::as_str)
    }

    /// How this key's title is drawn, when the page says something other than
    /// the app's default.
    ///
    /// `None` is not "invisible": it means "whatever the window's default style
    /// is", which is one setting for the keys that do not override it.
    pub fn title_style(&self, key: u16) -> Option<&TitleStyle> {
        self.title_styles.get(&key)
    }

    /// Sets how this key's title is drawn, or takes the override away.
    ///
    /// The size is clamped to the range a tile can hold, so a hand-written number
    /// cannot reach the drawing. `None` puts the key back on the app's default;
    /// it does not remove the title. A key with no title keeps no style either —
    /// [`PageKeys::set_title`] clears both together.
    pub fn set_title_style(&mut self, key: u16, style: Option<TitleStyle>) {
        match style {
            Some(style) => {
                self.title_styles.insert(key, style.clamped());
            }
            None => {
                self.title_styles.remove(&key);
            }
        }
    }

    /// Sets the short text this key shows, or removes it.
    ///
    /// The title is trimmed before it is stored. An empty or whitespace-only
    /// title removes the entry instead of storing an empty string, so "no
    /// title" has exactly one representation. A title longer than
    /// [`MAX_TITLE_CHARS`] characters, or holding a control character, is
    /// refused with a [`TitleError`] and leaves the key as it was.
    ///
    /// Titles are independent of [`PageKeys::appearance`] and of the bindings:
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
            // A key with no words has nothing to draw, so it keeps no style
            // either: "no title" stays one representation in the file.
            self.title_styles.remove(&key);
            return Ok(());
        };
        let title = title.trim();
        if title.is_empty() {
            self.titles.remove(&key);
            self.title_styles.remove(&key);
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

    /// Exchanges everything two keys do and show.
    ///
    /// The binding, the picture, the words and the words' style travel together,
    /// because they are one key: a swap that moved only the binding would leave a
    /// picture and a title naming something the key no longer runs. A key with
    /// nothing on it moves as an absence — nothing is copied and nothing is
    /// overwritten — so two keys trade places and both stay exactly as full as
    /// they were.
    pub fn swap_keys(&mut self, from: KeyNumber, to: KeyNumber) {
        if from == to {
            return;
        }
        self.bindings.swap(from, to);
        let from = u16::from(from.get());
        let to = u16::from(to.get());
        swap_entry(&mut self.appearance, from, to);
        swap_entry(&mut self.titles, from, to);
        swap_entry(&mut self.title_styles, from, to);
    }

    /// The colour for a key that shows one, or `None` when it shows an image or
    /// nothing of its own.
    pub fn key_color(&self, key_number: u16) -> Option<[u8; 3]> {
        self.appearance.get(&key_number).and_then(Appearance::rgb)
    }

    /// What this key shows, when the page says.
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

    /// Brings every stored style's size into the range a tile can hold.
    ///
    /// [`PageKeys::set_title_style`] clamps what it writes, so this is for a file
    /// someone edited by hand: a number a tile cannot hold would still be drawn
    /// (the fit steps down) but it would be a ceiling no title ever reaches, and
    /// the whole point of the field is that its number means something.
    fn clamp_title_styles(&mut self) {
        for style in self.title_styles.values_mut() {
            *style = style.clamped();
        }
    }

    /// Returns the slot name (white, red, etc.)
    pub fn slot_name(&self) -> &str {
        &self.slot
    }

    /// Rewrites every key that opens `from` so it opens `to`.
    ///
    /// Used when a folder is renamed: a page is addressed by name, and the key
    /// that opened it has to keep opening it. Nothing else on the page moves.
    fn retarget_page(&mut self, from: &str, to: &str) {
        self.retarget(
            |action| matches!(action, crate::action::Action::Folder { page } if page == from),
            || crate::action::Action::Folder {
                page: to.to_string(),
            },
        );
    }

    /// Rewrites every key that switches to `from` so it switches to `to`.
    ///
    /// Used when a profile is renamed: the name a `profile` binding holds is how
    /// one profile points at another, so the keys have to follow.
    fn retarget_profile(&mut self, from: &str, to: &str) {
        self.retarget(
            |action| matches!(action, crate::action::Action::Profile { name } if name == from),
            || crate::action::Action::Profile {
                name: to.to_string(),
            },
        );
    }

    /// Replaces the action of every key it matches, and nothing else.
    ///
    /// The keys are collected first: the replacement is built while the store is
    /// borrowed, and a key that does not match is never rewritten.
    fn retarget(
        &mut self,
        matches: impl Fn(&crate::action::Action) -> bool,
        replacement: impl Fn() -> crate::action::Action,
    ) {
        let keys: Vec<crate::key::KeyNumber> = self
            .bindings
            .bindings()
            .iter()
            .filter(|binding| matches(&binding.action))
            .map(|binding| binding.key)
            .collect();
        for key in keys {
            // A key of the store is already a valid key: this cannot fail, and
            // if it ever did the binding would be the one that was there.
            let _ = self.bindings.set(key, replacement());
        }
    }
}

/// The longest a page's name may be, in characters.
///
/// A page name is a label in a list and the word on the key that opens it, so it
/// is bounded like a title is. It is shorter than a title: a folder is a place,
/// and places are named with one word.
pub const MAX_PAGE_NAME_CHARS: usize = 24;

/// A profile with its name, its root page and its folders.
///
/// The deck shows one page at a time. Activating a profile starts at its root,
/// which is the page the profile has always been.
///
/// ## Why the read side is written by hand
///
/// The root page is written **flat**: a profile file has `bindings`, `appearance`
/// and `titles` at its own level, exactly as it did before pages existed, so no
/// migration step is needed. `#[serde(flatten)]` would say that in one attribute,
/// but it cannot read this shape back: flattening buffers the fields into an
/// untyped form, and serde then refuses to turn the keys of `appearance` and
/// `titles` — `"1"`, `"4"` — into the `u16` they address. So the writer is
/// derived with `flatten` and the reader goes through a helper that names every
/// field once.
#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    /// The profile name, shown in the UI.
    pub name: String,
    /// The page the deck shows when this profile is activated.
    ///
    /// Flattened on purpose: the root page **is** the profile's own keys, so a
    /// file written before pages existed loads as a profile whose root page is
    /// exactly what it had. No migration step and no version bump.
    #[serde(flatten)]
    pub root: PageKeys,
    /// The named pages under the root, which is what a folder opens.
    ///
    /// Absent from the file until one is created, so a profile with no folders
    /// is byte-for-byte the profile it was.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pages: BTreeMap<String, PageKeys>,
}

/// The shape a profile file has, named field by field so the reader can build a
/// root page out of it.
///
/// Every field is optional: a file written before a field existed simply does
/// not have it, and the default fills in — which is the same rule [`PageKeys`]
/// follows.
#[derive(Deserialize)]
struct ProfileIn {
    name: String,
    #[serde(default)]
    bindings: Option<ActionStore>,
    #[serde(default)]
    appearance: Option<BTreeMap<u16, Appearance>>,
    #[serde(default)]
    titles: Option<BTreeMap<u16, String>>,
    #[serde(default)]
    title_styles: Option<BTreeMap<u16, TitleStyle>>,
    #[serde(default)]
    colors: Option<BTreeMap<u16, [u8; 3]>>,
    #[serde(default)]
    slot: Option<String>,
    #[serde(default)]
    pages: BTreeMap<String, PageKeys>,
}

impl<'de> Deserialize<'de> for Profile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let file = ProfileIn::deserialize(deserializer)?;
        Ok(Self {
            name: file.name,
            root: PageKeys {
                bindings: file.bindings.unwrap_or_default(),
                appearance: file.appearance.unwrap_or_default(),
                titles: file.titles.unwrap_or_default(),
                title_styles: file.title_styles.unwrap_or_default(),
                colors: file.colors.unwrap_or_default(),
                slot: file.slot.unwrap_or_else(default_slot),
            },
            pages: file.pages,
        })
    }
}

impl Profile {
    /// Creates a new empty profile with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            // A profile with nothing on it is blank, not white: a key the user
            // has not touched should look like a key with nothing on it.
            root: PageKeys::with_slot("blank"),
            pages: BTreeMap::new(),
        }
    }

    /// Creates a profile whose root page shows these flat colours.
    pub fn with_solids(name: &str, slot: &str, solids: &[(u16, [u8; 3])]) -> Self {
        let mut root = PageKeys::with_slot(slot);
        root.appearance = solids
            .iter()
            .map(|(key, rgb)| (*key, Appearance::solid(*rgb)))
            .collect();
        Self {
            name: name.to_string(),
            root,
            pages: BTreeMap::new(),
        }
    }

    /// The page a name points at, or the root when there is no name.
    pub fn page(&self, name: Option<&str>) -> Option<&PageKeys> {
        match name {
            None => Some(&self.root),
            Some(name) => self.pages.get(name),
        }
    }

    /// The page a name points at, mutably, or the root when there is no name.
    pub fn page_mut(&mut self, name: Option<&str>) -> Option<&mut PageKeys> {
        match name {
            None => Some(&mut self.root),
            Some(name) => self.pages.get_mut(name),
        }
    }

    /// The names of the folders under the root, in name order.
    pub fn page_names(&self) -> impl Iterator<Item = &str> {
        self.pages.keys().map(String::as_str)
    }

    /// How many folders this profile has. The root page is not counted.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Adds an empty folder, answering whether it was created.
    ///
    /// A name that is empty, made only of spaces, longer than
    /// [`MAX_PAGE_NAME_CHARS`], carrying a control character, or already taken
    /// is refused: pages are addressed by name, so two of them cannot share one.
    pub fn add_page(&mut self, name: &str) -> bool {
        let name = name.trim();
        if !page_name_is_valid(name) || self.pages.contains_key(name) {
            return false;
        }
        // A new folder starts blank rather than inheriting the root's colours:
        // an empty page the user has not painted is a key with nothing on it.
        self.pages
            .insert(name.to_string(), PageKeys::with_slot("blank"));
        true
    }

    /// Removes a folder, and everything on it.
    ///
    /// Answers whether it existed. The caller is the one that knows what the
    /// deck is showing: a page removed while it is open leaves the deck on a
    /// name that no longer exists.
    pub fn remove_page(&mut self, name: &str) -> bool {
        self.pages.remove(name).is_some()
    }

    /// Renames a folder, answering whether the new name was free.
    ///
    /// Every key that opens it follows the rename, on every page. A page is
    /// addressed by its name, so a key still holding the old one would open
    /// nothing: the deck would answer "which this profile no longer has" for a
    /// folder that is right there.
    pub fn rename_page(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim();
        if from == to {
            return true;
        }
        if !page_name_is_valid(to) || self.pages.contains_key(to) {
            return false;
        }
        let Some(page) = self.pages.remove(from) else {
            return false;
        };
        self.pages.insert(to.to_string(), page);
        for keys in self.every_page_mut() {
            keys.retarget_page(from, to);
        }
        true
    }

    /// Every page this profile owns, root first.
    fn every_page_mut(&mut self) -> impl Iterator<Item = &mut PageKeys> {
        std::iter::once(&mut self.root).chain(self.pages.values_mut())
    }

    /// Rewrites every key that switches to `from` so it switches to `to`.
    fn retarget_profile(&mut self, from: &str, to: &str) {
        for keys in self.every_page_mut() {
            keys.retarget_profile(from, to);
        }
    }

    /// Migrates the legacy colour map on every page.
    fn migrate_legacy_colors(&mut self) {
        for page in self.every_page_mut() {
            page.migrate_legacy_colors();
        }
    }

    /// Clamps every stored title style on every page.
    fn clamp_title_styles(&mut self) {
        for page in self.every_page_mut() {
            page.clamp_title_styles();
        }
    }
}

/// Exchanges two entries of a key-addressed map, taking absence with them.
///
/// A missing key is not an error here: it is how "nothing on this key" is
/// spelled, and it is exactly what the swap has to move.
fn swap_entry<T>(map: &mut BTreeMap<u16, T>, from: u16, to: u16) {
    let from_value = map.remove(&from);
    let to_value = map.remove(&to);
    if let Some(value) = from_value {
        map.insert(to, value);
    }
    if let Some(value) = to_value {
        map.insert(from, value);
    }
}

/// Whether `name` can address a page.
fn page_name_is_valid(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_PAGE_NAME_CHARS
        && !name.chars().any(char::is_control)
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

    /// Renames a profile, and every key in every profile that switches to it.
    ///
    /// A profile is addressed by its name: that is what a `profile` binding
    /// holds, and it is how the active profile is remembered. Both follow, so a
    /// key that jumped to `stream` does not end up jumping to nothing.
    ///
    /// The file is written here because only this layer knows which profile's
    /// file it is: the window has the deck key and the folder, and nothing else.
    pub fn rename_profile(
        &mut self,
        from: &str,
        to: &str,
        deck_key: &str,
        config_dir: &Path,
    ) -> Result<(), String> {
        let to = to.trim();
        if from == to {
            return Ok(());
        }
        crate::action::Action::validate_profile_name(to)
            .map_err(|error| format!("a profile cannot be called that: {error}"))?;
        if self.profiles.contains_key(to) {
            return Err(format!("\"{to}\" is already a profile of this deck."));
        }
        let Some(mut profile) = self.profiles.remove(from) else {
            return Err(format!("no such profile: {from}"));
        };
        profile.name = to.to_string();
        self.profiles.insert(to.to_string(), profile);
        if self.active.as_deref() == Some(from) {
            self.active = Some(to.to_string());
        }
        for kept in self.profiles.values_mut() {
            kept.retarget_profile(from, to);
        }
        self._save_to(&crate::config::profiles_path_in(config_dir, deck_key))
    }

    /// Updates the bindings of one page of the active profile, and saves to disk.
    ///
    /// `page` is the page the deck is showing: `None` is the root. The window
    /// keeps one store for what the deck runs, and a page switch hands it a
    /// different one, so this is the call that writes what is on screen back
    /// where it belongs.
    pub fn update_bindings(
        &mut self,
        bindings: &ActionStore,
        page: Option<&str>,
        deck_key: &str,
        config_dir: &Path,
    ) -> Result<(), String> {
        let active_name = self.active.clone();
        if let Some(name) = active_name {
            if let Some(profile) = self.profiles.get_mut(&name) {
                match profile.page_mut(page) {
                    Some(keys) => keys.bindings = bindings.clone(),
                    // A page that no longer exists is not somewhere to write:
                    // the deck would keep running keys nobody can see again.
                    None => return Err(format!("no such page: {}", page.unwrap_or(""))),
                }
                return self._save_to(&crate::config::profiles_path_in(config_dir, deck_key));
            }
        }
        Ok(())
    }

    /// Returns the bindings of one page of the active profile.
    pub fn active_bindings(&self, page: Option<&str>) -> Option<ActionStore> {
        self.active()
            .and_then(|p| p.page(page))
            .map(|keys| keys.bindings.clone())
    }

    /// Merges one page's bindings from the active profile into `target`.
    pub fn apply_bindings(
        &self,
        target: &mut ActionStore,
        page: Option<&str>,
    ) -> Result<(), ActionError> {
        if let Some(keys) = self.active().and_then(|active| active.page(page)) {
            *target = keys.bindings.clone();
            Ok(())
        } else {
            Err(ActionError::ProfileError("no active profile".to_string()))
        }
    }

    /// Returns a copy of the active profile with its bindings and colors.
    pub fn active_profile(&self) -> Option<Profile> {
        self.active().cloned()
    }

    /// The active profile, to change it in place.
    ///
    /// The caller is the one that saves: creating a page is a change worth
    /// writing, and only the window knows the deck key and the folder that goes
    /// with it.
    pub fn active_mut(&mut self) -> Option<&mut Profile> {
        let name = self.active.clone()?;
        self.profiles.get_mut(&name)
    }

    /// The flat colours the active profile paints, for the keys that show one.
    ///
    /// A key showing an image is absent here on purpose: a caller that only
    /// knows about colours cannot paint over an image by accident.
    pub fn active_profile_colors(
        &self,
        page: Option<&str>,
    ) -> Option<(ActionStore, BTreeMap<u16, [u8; 3]>, String)> {
        let keys = self.active()?.page(page)?;
        let solids = keys
            .appearance
            .iter()
            .filter_map(|(key, appearance)| appearance.rgb().map(|rgb| (*key, rgb)))
            .collect();
        Some((keys.bindings.clone(), solids, keys.slot.clone()))
    }

    /// The appearance of every key the active profile says something about.
    pub fn active_profile_appearance(
        &self,
        page: Option<&str>,
    ) -> Option<BTreeMap<u16, Appearance>> {
        Some(self.active()?.page(page)?.appearance.clone())
    }

    /// How the active profile draws one key's title, when it overrides the
    /// default.
    pub fn title_style(&self, key: u16, page: Option<&str>) -> Option<&TitleStyle> {
        self.active()?.page(page)?.title_style(key)
    }

    /// Sets how the active profile draws one key's title, and saves to disk.
    ///
    /// The title itself is not touched: changing a key's font is not renaming
    /// it. `None` puts the key back on the app's default style.
    pub fn set_title_style(
        &mut self,
        key: u16,
        style: Option<TitleStyle>,
        page: Option<&str>,
        deck_key: &str,
        config_dir: &Path,
    ) -> Result<(), String> {
        let name = self
            .active
            .clone()
            .ok_or_else(|| "no active profile".to_string())?;
        let profile = self
            .profiles
            .get_mut(&name)
            .ok_or_else(|| format!("no such profile: {name}"))?;
        profile
            .page_mut(page)
            .ok_or_else(|| format!("no such page: {}", page.unwrap_or("")))?
            .set_title_style(key, style);
        self._save_to(&crate::config::profiles_path_in(config_dir, deck_key))
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
    pub fn set_title(
        &mut self,
        key: u16,
        title: Option<&str>,
        page: Option<&str>,
    ) -> Result<(), String> {
        let name = self
            .active
            .clone()
            .ok_or_else(|| "no active profile".to_string())?;
        let profile = self
            .profiles
            .get_mut(&name)
            .ok_or_else(|| format!("no such profile: {name}"))?;
        profile
            .page_mut(page)
            .ok_or_else(|| format!("no such page: {}", page.unwrap_or("")))?
            .set_title(key, title)
            .map_err(|error| error.to_string())
    }

    /// Exchanges two keys of one page, everything they do and show included.
    ///
    /// The page is the authority on what a key does and shows, so the swap is
    /// made there and written by the caller with [`ProfileStore::save`] — the
    /// same order [`ProfileStore::set_title`] and
    /// [`ProfileStore::set_appearance`] follow. A page that is no longer there
    /// is a sentence, never a silent no-op: the window would otherwise repaint
    /// two keys that did not move.
    ///
    /// # Errors
    ///
    /// A message naming the missing profile or page.
    pub fn swap_keys(
        &mut self,
        from: KeyNumber,
        to: KeyNumber,
        page: Option<&str>,
    ) -> Result<(), String> {
        let name = self
            .active
            .clone()
            .ok_or_else(|| "no active profile".to_string())?;
        let profile = self
            .profiles
            .get_mut(&name)
            .ok_or_else(|| format!("no such profile: {name}"))?;
        profile
            .page_mut(page)
            .ok_or_else(|| format!("no such page: {}", page.unwrap_or("")))?
            .swap_keys(from, to);
        Ok(())
    }

    pub fn set_appearance(
        &mut self,
        key: u16,
        appearance: Option<Appearance>,
        page: Option<&str>,
    ) -> Result<(), String> {
        let name = self
            .active
            .clone()
            .ok_or_else(|| "no active profile".to_string())?;
        let profile = self
            .profiles
            .get_mut(&name)
            .ok_or_else(|| format!("no such profile: {name}"))?;
        let keys = profile
            .page_mut(page)
            .ok_or_else(|| format!("no such page: {}", page.unwrap_or("")))?;
        match appearance {
            Some(appearance) => {
                keys.appearance.insert(key, appearance);
            }
            None => {
                keys.appearance.remove(&key);
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
            profile.clamp_title_styles();
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
    use crate::action::Action;
    use crate::assets::AssetName;
    use crate::key::KeyNumber;
    use crate::title::TitlePosition;

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
                profile.root.appearance.is_empty(),
                "{} must start blank, not painted",
                profile.name
            );
            assert_eq!(profile.root.slot_name(), "blank", "{}", profile.name);
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
            stream.root.appearance_for(1),
            Some(&Appearance::solid([255, 0, 0]))
        );
        assert_eq!(
            stream.root.appearance_for(4),
            Some(&Appearance::solid([255, 255, 0]))
        );
        assert!(
            stream.root.colors.is_empty(),
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
                .root
                .appearance_for(1)
                .and_then(Appearance::asset)
                .map(AssetName::as_str),
            Some("live.png"),
            "an explicit appearance outranks the legacy colour"
        );
        assert_eq!(stream.root.key_color(1), None);
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
            .root
            .appearance
            .insert(2, Appearance::image(name.clone()));
        store.save("deck", dir.path()).expect("save");

        let loaded = ProfileStore::load("deck", dir.path()).expect("load");
        let stream = loaded.active().expect("the active profile");

        assert_eq!(
            stream.root.appearance_for(2),
            Some(&Appearance::image(name))
        );
        assert_eq!(stream.root.key_color(2), None, "an image is not a colour");
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
        stream
            .root
            .appearance
            .insert(1, Appearance::solid([255, 0, 0]));
        stream
            .root
            .appearance
            .insert(5, Appearance::solid([255, 0, 255]));
        stream.root.appearance.insert(3, Appearance::image(name));

        let (_, solids, _) = store
            .active_profile_colors(None)
            .expect("the active profile");
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
            .set_appearance(1, Some(Appearance::image(name.clone())), None)
            .expect("set the picture");
        let stream = store.active().expect("the active profile");
        assert_eq!(
            stream.root.appearance_for(1),
            Some(&Appearance::image(name))
        );
        assert_eq!(
            stream.root.key_color(1),
            None,
            "a picture is not a colour to paint with"
        );

        store
            .set_appearance(1, None, None)
            .expect("remove the picture");
        assert_eq!(
            store
                .active()
                .expect("the active profile")
                .root
                .appearance_for(1),
            None,
            "removing the artwork leaves the key with no appearance of its own"
        );
    }

    #[test]
    fn a_title_can_be_set_read_and_removed() {
        let mut profile = Profile::new("stream");
        assert_eq!(profile.root.title(1), None, "a new profile titles nothing");

        profile
            .root
            .set_title(1, Some("Mute"))
            .expect("a short title");
        assert_eq!(profile.root.title(1), Some("Mute"));
        assert_eq!(
            profile.root.appearance_for(1),
            None,
            "a title is not an appearance"
        );

        profile.root.set_title(1, None).expect("remove the title");
        assert_eq!(profile.root.title(1), None);
        assert!(
            profile.root.titles.is_empty(),
            "removing the only title leaves the map empty, not holding a hole"
        );
    }

    #[test]
    fn a_title_is_stored_trimmed() {
        let mut profile = Profile::new("stream");
        profile
            .root
            .set_title(2, Some("  Live  "))
            .expect("a padded title");
        assert_eq!(profile.root.title(2), Some("Live"));
    }

    #[test]
    fn a_whitespace_only_title_removes_the_entry() {
        let mut profile = Profile::new("stream");
        profile
            .root
            .set_title(3, Some("Chat"))
            .expect("set a title");
        profile
            .root
            .set_title(3, Some("   \t "))
            .expect("whitespace is not a title");
        assert_eq!(profile.root.title(3), None);
        assert!(profile.root.titles.is_empty());
    }

    #[test]
    fn a_title_longer_than_the_cap_is_refused() {
        let mut profile = Profile::new("stream");
        let too_long = "a".repeat(MAX_TITLE_CHARS + 1);

        let error = profile
            .root
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
            profile.root.title(1),
            None,
            "a refused title leaves the key as it was"
        );
    }

    #[test]
    fn a_title_exactly_at_the_cap_is_kept() {
        let mut profile = Profile::new("stream");
        let longest = "a".repeat(MAX_TITLE_CHARS);
        profile
            .root
            .set_title(1, Some(&longest))
            .expect("exactly the cap fits");
        assert_eq!(profile.root.title(1), Some(longest.as_str()));
    }

    #[test]
    fn a_title_with_a_control_character_is_refused() {
        let mut profile = Profile::new("stream");

        let error = profile
            .root
            .set_title(1, Some("live\nnow"))
            .expect_err("a newline is not printable text");

        assert_eq!(error, TitleError::ControlCharacter);
        assert_eq!(
            error.to_string(),
            "a title cannot contain control characters"
        );
        assert_eq!(profile.root.title(1), None);
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
            stream.root.title(1),
            None,
            "an absent titles map means no key has a title"
        );
        assert!(stream.root.titles.is_empty());
        assert_eq!(
            stream.root.appearance_for(1),
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
            profile.root.set_title(1, Some("Mute")).expect("set");
            profile.root.set_title(7, Some("Cam")).expect("set");
        }
        store.save("deck", dir.path()).expect("save");

        let loaded = ProfileStore::load("deck", dir.path()).expect("load");
        let stream = loaded.active().expect("the active profile");

        assert_eq!(stream.root.title(1), Some("Mute"));
        assert_eq!(stream.root.title(7), Some("Cam"));
        assert_eq!(
            stream.root.title(2),
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
    fn a_swap_moves_a_keys_binding_picture_and_words_together() {
        // A key is one thing: what it runs, what it shows and what it says. A
        // swap that moved only the binding would leave a picture and a title
        // naming something the key no longer runs.
        let mut keys = PageKeys::default();
        let one = KeyNumber::new(1).expect("key 1");
        let seven = KeyNumber::new(7).expect("key 7");
        let mute = Action::keys("ctrl+m").expect("a combination");
        let docs = Action::OpenUrl {
            url: "https://example.com".to_string(),
        };
        keys.bindings.set(one, mute.clone()).expect("bind");
        keys.bindings.set(seven, docs.clone()).expect("bind");
        keys.set_title(1, Some("Mute")).expect("title");
        keys.set_title(7, Some("Docs")).expect("title");
        keys.appearance
            .insert(1, Appearance::Solid { rgb: [1, 2, 3] });
        keys.appearance
            .insert(7, Appearance::Solid { rgb: [4, 5, 6] });
        let loud = TitleStyle {
            family: "Georgia".to_string(),
            size_px: 24,
            position: TitlePosition::Top,
            uppercase: true,
        };
        keys.set_title_style(1, Some(loud.clone()));

        keys.swap_keys(one, seven);

        assert_eq!(keys.bindings.get(one), Some(&docs));
        assert_eq!(keys.bindings.get(seven), Some(&mute));
        assert_eq!(keys.title(1), Some("Docs"));
        assert_eq!(keys.title(7), Some("Mute"));
        assert_eq!(
            keys.appearance_for(1),
            Some(&Appearance::Solid { rgb: [4, 5, 6] })
        );
        assert_eq!(
            keys.appearance_for(7),
            Some(&Appearance::Solid { rgb: [1, 2, 3] })
        );
        assert_eq!(
            keys.title_style(7),
            Some(&loud.clamped()),
            "a key's own style travels with the words it styles"
        );
        assert_eq!(keys.title_style(1), None, "and is not copied onto both");
    }

    #[test]
    fn a_swap_does_not_invent_what_the_empty_key_did_not_have() {
        let mut keys = PageKeys::default();
        let two = KeyNumber::new(2).expect("key 2");
        let nine = KeyNumber::new(9).expect("key 9");
        keys.bindings
            .set(two, Action::keys("ctrl+m").expect("a combination"))
            .expect("bind");
        keys.set_title(2, Some("Mute")).expect("title");

        keys.swap_keys(two, nine);

        assert_eq!(keys.bindings.get(two), None);
        assert!(keys.bindings.get(nine).is_some());
        assert_eq!(keys.title(2), None);
        assert_eq!(keys.title(9), Some("Mute"));
        assert_eq!(keys.bindings.len(), 1, "nothing was copied");
    }

    #[test]
    fn a_swap_survives_a_save_and_a_load() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = ProfileStore::new();
        let one = KeyNumber::new(1).expect("key 1");
        let four = KeyNumber::new(4).expect("key 4");
        {
            let page = store
                .active_mut()
                .expect("the active profile")
                .page_mut(None)
                .expect("the root");
            page.bindings
                .set(one, Action::keys("ctrl+m").expect("a combination"))
                .expect("bind");
            page.bindings
                .set(
                    four,
                    Action::OpenUrl {
                        url: "https://example.com".to_string(),
                    },
                )
                .expect("bind");
            page.set_title(1, Some("Mute")).expect("title");
        }
        store.swap_keys(one, four, None).expect("swap");
        store.save("deck", dir.path()).expect("save");

        let read = ProfileStore::load("deck", dir.path()).expect("load");
        let root = &read.active().expect("the profile").root;
        assert_eq!(
            root.bindings.get(one).map(Action::summary).as_deref(),
            Some("OPEN example.com")
        );
        assert_eq!(
            root.bindings.get(four).map(Action::summary).as_deref(),
            Some("Ctrl+M")
        );
        assert_eq!(root.title(1), None);
        assert_eq!(root.title(4), Some("Mute"));
    }

    #[test]
    fn a_title_is_written_under_its_key_number() {
        let mut profile = Profile::new("stream");
        profile.root.set_title(3, Some("Chat")).expect("set");
        let json = serde_json::to_string(&profile).expect("serialize");
        assert!(
            json.contains(r#""titles":{"3":"Chat"}"#),
            "titles are a key-to-text object: {json}"
        );
    }

    #[test]
    fn one_key_can_draw_its_title_differently_from_another() {
        // The style is per key, so two keys of one profile can disagree about
        // the font, the size, where the words sit and their case.
        let mut profile = Profile::new("stream");
        profile.root.set_title(1, Some("Chat")).expect("set");
        profile.root.set_title(2, Some("Cam")).expect("set");
        let loud = TitleStyle {
            family: "Georgia".to_string(),
            size_px: 24,
            position: TitlePosition::Top,
            uppercase: true,
        };

        profile.root.set_title_style(2, Some(loud.clone()));

        assert_eq!(
            profile.root.title_style(1),
            None,
            "a key with no override uses the app's default"
        );
        assert_eq!(profile.root.title_style(2), Some(&loud));
        assert_eq!(
            profile.root.title(2),
            Some("Cam"),
            "the words are untouched"
        );
    }

    #[test]
    fn a_title_style_survives_a_save_and_a_load() {
        let dir = tempfile::tempdir().expect("temp dir");
        let style = TitleStyle {
            family: "Segoe UI".to_string(),
            size_px: 18,
            position: TitlePosition::Middle,
            uppercase: false,
        };
        let mut store = ProfileStore::new();
        store
            .set_title(2, Some("Mute"), None)
            .expect("set the title");
        store
            .set_title_style(2, Some(style.clone()), None, "deck", dir.path())
            .expect("set the style");

        let read = ProfileStore::load("deck", dir.path()).expect("load");
        let profile = read.active().expect("the profile");
        assert_eq!(profile.root.title(2), Some("Mute"));
        assert_eq!(profile.root.title_style(2), Some(&style));
    }

    #[test]
    fn a_profile_written_before_title_styles_loads_unchanged() {
        // The field is absent from every file this build wrote before today.
        let text = r#"{
            "profiles": {
                "default": {
                    "name": "default",
                    "bindings": {"bindings": []},
                    "titles": {"1": "Chat"},
                    "slot": "blank"
                }
            },
            "active": "default"
        }"#;
        let store: ProfileStore = serde_json::from_str(text).expect("an old profile");
        let profile = store.active().expect("the active profile");
        assert_eq!(profile.root.title(1), Some("Chat"));
        assert_eq!(
            profile.root.title_style(1),
            None,
            "no style means the app's default"
        );
    }

    #[test]
    fn re_saving_a_title_keeps_the_style_it_was_drawn_in() {
        // Renaming a key is not restyling it: the editor's SAVE TITLE runs
        // `set_title` again, and that must leave the style alone.
        let mut profile = Profile::new("stream");
        let style = TitleStyle {
            family: "Georgia".to_string(),
            size_px: 24,
            position: TitlePosition::Top,
            uppercase: true,
        };
        profile.root.set_title(2, Some("Cam")).expect("set");
        profile.root.set_title_style(2, Some(style.clone()));

        profile.root.set_title(2, Some("Camera")).expect("rename");

        assert_eq!(profile.root.title(2), Some("Camera"));
        assert_eq!(
            profile.root.title_style(2),
            Some(&style),
            "the words changed, not the font"
        );
    }

    #[test]
    fn a_style_edited_by_hand_is_clamped_on_the_way_in() {
        // The writer clamps; the reader has to as well, or a hand-edited number
        // would be a ceiling no title ever reaches.
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = ProfileStore::new();
        store.set_title(1, Some("Chat"), None).expect("set");
        let path = crate::config::profiles_path_in(dir.path(), "deck");
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("dir");
        std::fs::write(
            &path,
            r#"{
                "profiles": {
                    "default": {
                        "name": "default",
                        "bindings": {"bindings": []},
                        "titles": {"1": "Chat"},
                        "title_styles": {"1": {"size_px": 400}},
                        "slot": "blank"
                    }
                },
                "active": "default"
            }"#,
        )
        .expect("write");

        let read = ProfileStore::load("deck", dir.path()).expect("load");
        let profile = read.active().expect("the profile");
        assert_eq!(
            profile.root.title_style(1).map(|style| style.size_px),
            Some(crate::title::MAX_SIZE_PX)
        );
    }

    #[test]
    fn renaming_a_profile_moves_it_and_the_keys_that_open_it() {
        // A profile is addressed by its name: it is what a `profile` binding
        // holds and what the active profile is remembered as. All of it moves,
        // on every page, and the file keeps the new name.
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = ProfileStore::new();
        store
            .profiles
            .insert("mine".to_string(), Profile::new("mine"));
        store.activate("mine").expect("activate");
        assert!(store.active_mut().expect("active").add_page("Media"));

        let opens_stream = Action::Profile {
            name: "stream".to_string(),
        };
        store
            .active_mut()
            .expect("active")
            .root
            .bindings
            .set(KeyNumber::new(1).expect("key 1"), opens_stream.clone())
            .expect("bind the root");
        store
            .active_mut()
            .expect("active")
            .page_mut(Some("Media"))
            .expect("the page")
            .bindings
            .set(KeyNumber::new(3).expect("key 3"), opens_stream)
            .expect("bind the page");

        store
            .rename_profile("stream", "Live", "deck", dir.path())
            .expect("rename");

        assert!(store.names().contains(&"Live"), "the profile moved");
        assert!(
            !store.names().contains(&"stream"),
            "and the old name is free"
        );
        assert_eq!(store.active_name(), Some("mine"), "the active one stayed");

        let opens_live = Action::Profile {
            name: "Live".to_string(),
        };
        let mine = store.active().expect("mine");
        assert_eq!(
            mine.root.bindings.get(KeyNumber::new(1).expect("key 1")),
            Some(&opens_live),
            "a key that switched to it follows the rename"
        );
        assert_eq!(
            mine.page(Some("Media"))
                .expect("the page")
                .bindings
                .get(KeyNumber::new(3).expect("key 3")),
            Some(&opens_live),
            "and so does one on a folder"
        );

        // Renaming the active profile moves what is active, and the result is
        // written: a reload answers the new name.
        store.activate("Live").expect("activate the renamed one");
        store
            .rename_profile("Live", "Live show", "deck", dir.path())
            .expect("rename the active one");
        assert_eq!(store.active_name(), Some("Live show"));

        let read = ProfileStore::load("deck", dir.path()).expect("load");
        assert_eq!(read.active_name(), Some("Live show"));
        assert!(read.names().contains(&"Live show"));

        // Two profiles cannot answer to one name, and an empty name addresses
        // nothing.
        assert!(store
            .rename_profile("mine", "Live show", "deck", dir.path())
            .is_err());
        assert!(store
            .rename_profile("mine", "   ", "deck", dir.path())
            .is_err());
        assert_eq!(
            store.active_name(),
            Some("Live show"),
            "a refusal changes nothing"
        );
        assert!(store.names().contains(&"mine"));
    }

    #[test]
    fn renaming_a_page_moves_the_keys_that_open_it() {
        // S6: a rename is not only the folder's name. A page is addressed by
        // name, so every key in the profile that opened it has to follow, on
        // every page, and the keys that opened something else must not move.
        let mut profile = Profile::new("stream");
        assert!(profile.add_page("Media"));
        assert!(profile.add_page("Live"));

        let opens_media = Action::Folder {
            page: "Media".to_string(),
        };
        profile
            .root
            .bindings
            .set(KeyNumber::new(4).expect("key 4"), opens_media.clone())
            .expect("bind the root");
        // A folder can open a sibling: the rename reaches those keys too.
        profile
            .page_mut(Some("Live"))
            .expect("the page")
            .bindings
            .set(KeyNumber::new(1).expect("key 1"), opens_media)
            .expect("bind the page");
        let unrelated = Action::keys("ctrl+m").expect("a combination");
        profile
            .root
            .bindings
            .set(KeyNumber::new(1).expect("key 1"), unrelated.clone())
            .expect("bind the root");

        assert!(profile.rename_page("Media", "Clips"));
        assert!(profile.page(Some("Clips")).is_some(), "the folder moved");
        assert!(
            profile.page(Some("Media")).is_none(),
            "and the old name is free"
        );

        let opens_clips = Action::Folder {
            page: "Clips".to_string(),
        };
        assert_eq!(
            profile.root.bindings.get(KeyNumber::new(4).expect("key 4")),
            Some(&opens_clips),
            "the root's key follows the rename"
        );
        assert_eq!(
            profile
                .page(Some("Live"))
                .expect("the page")
                .bindings
                .get(KeyNumber::new(1).expect("key 1")),
            Some(&opens_clips),
            "and so does a key on another page"
        );
        assert_eq!(
            profile.root.bindings.get(KeyNumber::new(1).expect("key 1")),
            Some(&unrelated),
            "a key that opens nothing was not touched"
        );
    }

    #[test]
    fn a_page_keeps_its_own_bindings_through_a_save_and_a_load() {
        // The path the pages card takes: a new page, a key bound on it, saved.
        // The root must come back exactly as it was, and the page must come back
        // with its key.
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = ProfileStore::new();
        let root_action = Action::keys("ctrl+m").expect("a combination");
        store
            .active_mut()
            .expect("the active profile")
            .root
            .bindings
            .set(KeyNumber::new(1).expect("key 1"), root_action.clone())
            .expect("bind the root");

        store
            .active_mut()
            .expect("the active profile")
            .add_page("Media");
        let page_action = Action::Folder {
            page: "Media".to_string(),
        };
        let mut media = ActionStore::new();
        media
            .set(KeyNumber::new(4).expect("key 4"), page_action.clone())
            .expect("bind the page");
        store
            .update_bindings(&media, Some("Media"), "deck", dir.path())
            .expect("save the page");

        let read = ProfileStore::load("deck", dir.path()).expect("load");
        let profile = read.active().expect("the profile");
        assert_eq!(
            profile.root.bindings.get(KeyNumber::new(1).expect("key 1")),
            Some(&root_action),
            "the root kept its key"
        );
        let page = profile.page(Some("Media")).expect("the page");
        assert_eq!(
            page.bindings.get(KeyNumber::new(4).expect("key 4")),
            Some(&page_action),
            "the page kept the key that opens it"
        );
        assert_eq!(
            profile.root.bindings.get(KeyNumber::new(4).expect("key 4")),
            None,
            "and the root never saw it"
        );
    }

    #[test]
    fn a_page_is_its_own_set_of_keys() {
        // S4: the same physical key may mean two things on two pages of one
        // profile, and the root is untouched by what a folder holds.
        let mut profile = Profile::new("stream");
        profile.root.set_title(1, Some("Root")).expect("root title");
        profile.add_page("Media");

        let media = profile.page_mut(Some("Media")).expect("the page");
        media.set_title(1, Some("Song")).expect("page title");
        media.set_title(4, Some("Next")).expect("page title");

        assert_eq!(profile.root.title(1), Some("Root"));
        assert_eq!(profile.root.title(4), None, "the root never saw key 4");
        assert_eq!(
            profile.page(Some("Media")).and_then(|page| page.title(1)),
            Some("Song")
        );
    }

    #[test]
    fn pages_survive_a_save_and_a_load_without_touching_the_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = ProfileStore::new();
        store
            .set_title(2, Some("Root key"), None)
            .expect("root title");
        {
            let profile = store.profiles.get_mut("default").expect("the profile");
            assert!(profile.add_page("Media"));
            profile
                .page_mut(Some("Media"))
                .expect("the page")
                .set_title(2, Some("Page key"))
                .expect("page title");
        }
        store.save("deck", dir.path()).expect("save");

        let read = ProfileStore::load("deck", dir.path()).expect("load");
        let profile = read.active().expect("the profile");
        assert_eq!(profile.root.title(2), Some("Root key"));
        assert_eq!(
            profile.page(Some("Media")).and_then(|page| page.title(2)),
            Some("Page key")
        );
    }

    #[test]
    fn a_profile_with_no_folders_writes_no_pages_key() {
        // S5: a profile without folders is byte-for-byte what it was, so a file
        // this build writes and one the previous build wrote are the same shape.
        let profile = Profile::new("stream");
        let json = serde_json::to_string(&profile).expect("serialize");
        assert!(
            !json.contains("\"pages\""),
            "an empty page map must not be written: {json}"
        );
        assert!(json.contains("\"name\":\"stream\""), "{json}");
        assert!(json.contains("\"bindings\""), "{json}");
    }

    #[test]
    fn a_page_name_has_to_be_usable_and_free() {
        let mut profile = Profile::new("stream");
        assert!(profile.add_page("Media"));
        assert!(!profile.add_page("Media"), "a name addresses one page");
        assert!(!profile.add_page("   "), "a blank name is no name");
        assert!(!profile.add_page(""), "an empty name is no name");
        assert!(
            !profile.add_page(&"x".repeat(MAX_PAGE_NAME_CHARS + 1)),
            "a name longer than the cap is refused"
        );
        assert!(
            !profile.add_page("bad\ttab"),
            "a control character is refused"
        );

        assert!(profile.rename_page("Media", "Music"));
        assert_eq!(profile.page_names().collect::<Vec<_>>(), vec!["Music"]);
        assert!(profile.remove_page("Music"));
        assert_eq!(profile.page_count(), 0);
    }

    #[test]
    fn taking_the_words_away_takes_their_style_too() {
        // A key with no title has nothing to draw, so it keeps no style: "no
        // title" stays one representation in the file.
        let mut profile = Profile::new("stream");
        profile.root.set_title(4, Some("Mute")).expect("set");
        profile
            .root
            .set_title_style(4, Some(TitleStyle::default().with_size(30)));

        profile.root.set_title(4, None).expect("clear");

        assert_eq!(profile.root.title(4), None);
        assert_eq!(profile.root.title_style(4), None);
        let json = serde_json::to_string(&profile).expect("serialize");
        assert!(
            !json.contains("title_styles"),
            "an empty style map must not be written: {json}"
        );
    }

    #[test]
    fn a_style_set_on_a_key_is_clamped_to_the_pixels_a_tile_can_hold() {
        // A hand-written number must not reach the drawing.
        let mut profile = Profile::new("stream");
        profile.root.set_title_style(
            1,
            Some(TitleStyle {
                size_px: 4_000,
                ..TitleStyle::default()
            }),
        );
        assert_eq!(
            profile.root.title_style(1).map(|style| style.size_px),
            Some(crate::title::MAX_SIZE_PX)
        );
    }
}

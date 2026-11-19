//! # ss550-engine
//!
//! The action layer of the StreamCraft SS550 controller: it binds the deck's
//! physical keys to actions, keeps those bindings on disk, and runs them
//! without ever going through a shell.
//!
//! ## What it does
//!
//! - [`Action`] — the catalogue of what a key can do: run a program
//!   ([`Action::Launch`]), open a URL ([`Action::OpenUrl`]), or send a key
//!   combination ([`Action::Keys`]).
//! - [`KeyCombo`] — a keystroke such as `Ctrl+Shift+M`, validated when it is
//!   built. Sending it needs a [`KeySender`], which the engine does not own.
//! - [`KeyNumber`] — a deck key addressed the way the UI shows it: 1, 2, 3,
//!   up to the deck's own key count. The engine keeps no upper bound; whether a
//!   key exists is the geometry's answer.
//! - [`ActionStore`] — the bindings, saved as JSON and loaded on startup.
//! - [`Recents`] — the actions used recently on one deck, a convenience the
//!   window can offer again. The bindings remain the source of truth.
//! - [`ActionRunner`] — runs actions on a background thread so the UI never
//!   waits for one.
//!
//! ## Example
//!
//! ```
//! use ss550_engine::{Action, ActionStore, KeyNumber};
//!
//! let mut store = ActionStore::new();
//! store
//!     .set(
//!         KeyNumber::new(1)?,
//!         Action::OpenUrl {
//!             url: "https://twitch.tv/dashboard".to_string(),
//!         },
//!     )
//!     .expect("valid action");
//!
//! assert_eq!(store.len(), 1);
//! # Ok::<(), ss550_engine::ActionError>(())
//! ```

#![deny(unsafe_code)]

pub mod action;
pub mod appearance;
pub mod assets;
pub mod config;
pub mod error;
pub mod key;
pub mod keys;
pub mod profiles;
pub mod recents;
pub mod runner;
pub mod store;
pub mod title;

pub use action::{is_shell_address, validate_url, Action};
pub use appearance::Appearance;
pub use assets::{Asset, AssetError, AssetLibrary, AssetName, ASSETS_DIR};
pub use config::{bindings_path, bindings_path_for, config_dir, recents_path_for};
pub use error::{ActionError, KeyComboError, TitleError};
pub use key::KeyNumber;
pub use keys::{BaseKey, KeyCombo, Modifier, NamedKey, MAX_SUMMARY_CHARS};
pub use profiles::{
    create_default_profiles, PageKeys, Profile, ProfileStore, MAX_PAGE_NAME_CHARS, MAX_TITLE_CHARS,
};
pub use recents::{Recents, MAX_RECENTS};
pub use runner::{command_for, run_action, ActionRunner, CommandSpec, KeySender};
pub use store::{ActionStore, Binding, STORE_VERSION};
pub use title::{TitlePosition, TitleStyle, DEFAULT_SIZE_PX, MAX_SIZE_PX, MIN_SIZE_PX};

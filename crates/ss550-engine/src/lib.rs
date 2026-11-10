//! # ss550-engine
//!
//! The action layer of the StreamCraft SS550 controller: it binds the deck's
//! physical keys to actions, keeps those bindings on disk, and runs them
//! without ever going through a shell.
//!
//! ## What it does
//!
//! - [`Action`] — the catalogue of what a key can do: run a program
//!   ([`Action::Launch`]) or open a URL ([`Action::OpenUrl`]).
//! - [`KeyNumber`] — a deck key addressed the way the UI shows it, 1 to 15.
//! - [`ActionStore`] — the bindings, saved as JSON and loaded on startup.
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
pub mod config;
pub mod error;
pub mod key;
pub mod runner;
pub mod store;

pub use action::{validate_url, Action};
pub use config::{bindings_path, config_dir};
pub use error::ActionError;
pub use key::KeyNumber;
pub use runner::{command_for, run_action, ActionRunner, CommandSpec};
pub use store::{ActionStore, Binding, STORE_VERSION};

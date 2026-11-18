//! Errors raised by the action layer.

use thiserror::Error;

/// Everything that can go wrong around an action or its bindings.
#[derive(Debug, Error)]
pub enum ActionError {
    /// The key number is not a valid 1-based number.
    #[error("key number {0} is out of range (it must be at least 1)")]
    KeyOutOfRange(u8),

    /// A launch action has no program to run.
    #[error("the program path is empty")]
    EmptyProgram,

    /// The URL is not a URI: it has no usable scheme.
    #[error("{0:?} is not a URL: it needs a scheme, for example https://example.com")]
    InvalidUrl(String),

    /// A navigation action names a page with nothing in it.
    #[error("a page name cannot be empty")]
    EmptyPageName,

    /// A navigation action names a page longer than a page name may be.
    #[error("a page name can hold at most {max} characters")]
    PageNameTooLong {
        /// The cap, in characters.
        max: usize,
    },

    /// A navigation action names a page with a character a name cannot hold.
    #[error("a page name cannot contain control characters")]
    PageNameControlCharacter,

    /// A plugin switch names no profile.
    #[error("a profile name cannot be empty")]
    EmptyProfileName,

    /// A plugin binding names a plugin id the contract refuses.
    #[error("the plugin id is not usable: {source}")]
    PluginId {
        /// Why the contract refused it.
        source: ss550_plugin_api::IdError,
    },

    /// A plugin binding names a cell id the contract refuses.
    #[error("the plugin cell is not usable: {source}")]
    PluginCell {
        /// Why the contract refused it.
        source: ss550_plugin_api::IdError,
    },

    /// A plugin reading was handed to the runner.
    ///
    /// A reading is drawn on the key and refreshed by the window; there is
    /// nothing to start, and a worker with no window cannot draw one.
    #[error("{action} is a reading the window draws, not something to run")]
    ReadingIsNotRunnable {
        /// The action, as its one-line label.
        action: String,
    },

    /// A navigation action was handed to the runner.
    ///
    /// Navigation is what the deck is *showing*, not something a process does:
    /// the window applies it where the keys are drawn, and the runner refuses it
    /// so a bound key can never half-run one.
    #[error("{action} changes what the deck shows, so the window applies it; the runner cannot")]
    NavigationIsNotRunnable {
        /// The action, as its one-line label.
        action: String,
    },

    /// The program could not be started.
    #[error("could not run {program}: {source}")]
    Launch {
        /// The program that failed to start.
        program: String,
        /// The operating system's error.
        source: std::io::Error,
    },

    /// The URL handler could not be started.
    #[error("could not open {url}: {source}")]
    Open {
        /// The URL that failed to open.
        url: String,
        /// The operating system's error.
        source: std::io::Error,
    },

    /// The bindings file could not be read.
    #[error("could not read {path}: {source}")]
    Read {
        /// The file that failed to read.
        path: String,
        /// The operating system's error.
        source: std::io::Error,
    },

    /// The bindings file could not be written.
    #[error("could not write {path}: {source}")]
    Write {
        /// The file that failed to write.
        path: String,
        /// The operating system's error.
        source: std::io::Error,
    },

    /// The bindings file is not valid JSON, or does not match the format.
    #[error("the bindings file is not valid: {source}")]
    InvalidJson {
        /// The parse error.
        source: serde_json::Error,
    },

    /// The bindings file was written by a newer version of the app.
    #[error(
        "the bindings file uses format version {found}, but this build understands {expected}"
    )]
    UnsupportedVersion {
        /// The version found in the file.
        found: u32,
        /// The version this build understands.
        expected: u32,
    },

    /// The bindings could not be serialised.
    #[error("could not serialise the bindings: {source}")]
    Serialise {
        /// The serialisation error.
        source: serde_json::Error,
    },

    /// The background runner is no longer accepting actions.
    #[error("the action runner has stopped")]
    RunnerStopped,

    /// The keystroke text is not a combination this app can send.
    #[error(transparent)]
    Keys(#[from] KeyComboError),

    /// A key combination needs the platform sender, and none was installed.
    ///
    /// The engine cannot type: delivering a keystroke is a platform call, and
    /// the crate that owns it hands in a [`KeySender`](crate::KeySender). A
    /// runner without one refuses the action instead of reporting a send that
    /// never happened.
    #[error("cannot send {keys}: no key sender is installed")]
    NeedsKeySender {
        /// The combination, as a key tile writes it.
        keys: String,
    },

    /// A key combination is sent, not started, so it has no program to run.
    #[error("{keys} is a key combination, not a program: it is sent, not launched")]
    KeysNotAProgram {
        /// The combination, as a key tile writes it.
        keys: String,
    },

    /// The platform sender could not deliver the combination.
    #[error("could not send {keys}: {reason}")]
    KeySend {
        /// The combination, as a key tile writes it.
        keys: String,
        /// What the platform sender reported.
        reason: String,
    },

    /// A profile operation failed for a named reason.
    #[error("{0}")]
    ProfileError(String),
}

/// Why a key title was refused.
///
/// A title is text the user typed, so a refusal is a sentence they can act on:
/// each variant says what a title must be instead. It is a type of its own
/// rather than a variant of [`ActionError`] because a title belongs to the
/// profile, not to an action, and because a text rule is worth comparing: the
/// tests assert the exact variant, while `ActionError` carries I/O and serde
/// errors that cannot be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TitleError {
    /// The title is longer than a key can show.
    #[error("a title can hold at most {max} characters")]
    TooLong {
        /// The cap, in characters.
        max: usize,
    },

    /// The title holds a character that is not printable text.
    #[error("a title cannot contain control characters")]
    ControlCharacter,
}

/// Why a keystroke combination was refused.
///
/// Every variant names the part that is wrong, not just that something is:
/// these messages are shown as they are, and a user who typed the combination
/// is the one who can fix it.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum KeyComboError {
    /// The text holds nothing at all.
    #[error("the key combination is empty")]
    Empty,

    /// A plus sign with nothing on one side of it.
    #[error("{combo:?} has an empty step: check the plus signs")]
    EmptyStep {
        /// The text as it was typed.
        combo: String,
    },

    /// Modifiers and no key to press.
    #[error("{combo:?} names modifiers but no key: a combination presses one key")]
    NoKey {
        /// The text as it was typed.
        combo: String,
    },

    /// Two keys in one combination.
    #[error("{combo:?} names two keys ({first} and {second}): a combination presses one")]
    TwoKeys {
        /// The text as it was typed.
        combo: String,
        /// The key that was already there.
        first: String,
        /// The key that is one too many.
        second: String,
    },

    /// The same modifier twice.
    #[error("{modifier} is listed twice in {combo:?}")]
    DuplicateModifier {
        /// The text as it was typed.
        combo: String,
        /// The modifier that repeats.
        modifier: String,
    },

    /// A part in a modifier's place that is not a modifier.
    #[error("{name:?} is not a modifier: ctrl, alt, shift and win are")]
    UnknownModifier {
        /// The part that was refused.
        name: String,
    },

    /// A part in the key's place that this app cannot send.
    #[error("{name:?} is not a key this app can send")]
    UnknownKey {
        /// The part that was refused.
        name: String,
    },
}

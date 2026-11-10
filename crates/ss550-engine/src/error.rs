//! Errors raised by the action layer.

use thiserror::Error;

/// Everything that can go wrong around an action or its bindings.
#[derive(Debug, Error)]
pub enum ActionError {
    /// The key number is outside the 15 keys the deck has.
    #[error("key number {0} is out of range (1-15)")]
    KeyOutOfRange(u8),

    /// A launch action has no program to run.
    #[error("the program path is empty")]
    EmptyProgram,

    /// The URL is not a URI: it has no usable scheme.
    #[error("{0:?} is not a URL: it needs a scheme, for example https://example.com")]
    InvalidUrl(String),

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
}

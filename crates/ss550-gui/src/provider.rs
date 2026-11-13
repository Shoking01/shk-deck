//! Opening the deck this build knows how to reach.
//!
//! This is the one place that names the concrete provider. Everything above
//! it — the window, the session, the bindings panel — sees a `Deck` and asks
//! it what it can do.
//!
//! The attempt is a thread of its own because opening a deck blocks for
//! seconds while its boot re-enumeration settles.

use std::time::Duration;

use ss550_driver::{Ss550Deck, Ss550Error};

/// Why an attempt to open the deck did not produce one.
///
/// The window keeps two states apart on purpose: a desk with no deck on it is
/// a normal situation the app waits out, while a deck that answered and then
/// failed is a problem worth showing. Losing that difference would turn every
/// unplugged deck into an error message.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// No deck answered on the bus.
    #[error("{0}")]
    NotFound(String),

    /// The deck answered, but it could not be opened or spoken to.
    #[error("{0}")]
    Other(String),
}

/// Opens the deck, waiting up to `timeout` for its boot to settle.
///
/// The returned deck has not been woken yet: the first paint, clear or
/// brightness change wakes it, once, through the seam's own idempotent
/// `initialize`. A deck that is never painted is never woken.
pub fn connect(timeout: Duration) -> Result<Ss550Deck, ConnectError> {
    Ss550Deck::connect_with_timeout(timeout).map_err(ConnectError::from)
}

impl From<Ss550Error> for ConnectError {
    fn from(error: Ss550Error) -> Self {
        match error {
            Ss550Error::DeviceNotFound { .. } => Self::NotFound(error.to_string()),
            other => Self::Other(other.to_string()),
        }
    }
}

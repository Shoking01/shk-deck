//! Why a heartbeat did, or did not, reach a deck.
//!
//! A deck that resets itself when the host goes quiet needs a report every few
//! seconds, and the report is worthless if the caller cannot tell whether it was
//! sent. A heartbeat that silently declines — because the deck was never woken,
//! or because an image transfer is queued — looks exactly like one that is
//! working, right up to the moment the deck drops off the bus with nothing in
//! the log to say why.
//!
//! [`HeartbeatState`] is that missing answer:
//! [`Deck::heartbeat_state`](crate::Deck::heartbeat_state) reports what the next
//! [`Deck::keep_alive`](crate::Deck::keep_alive) would do, and names the reason
//! when it would do nothing.
//!
//! ```
//! use deck_api::HeartbeatState;
//!
//! // A deck that cannot report anything says so, instead of claiming health.
//! assert_eq!(HeartbeatState::default(), HeartbeatState::Unreported);
//! assert!(HeartbeatState::Asleep.is_blocked());
//! assert!(HeartbeatState::ImagePending.is_blocked());
//! assert!(!HeartbeatState::Due.is_blocked());
//! assert!(!HeartbeatState::RecentWrite.is_blocked());
//! ```

/// What the next [`Deck::keep_alive`](crate::Deck::keep_alive) would do, and why.
///
/// The reason matters as much as the answer: `Ok` from a heartbeat that wrote
/// nothing is indistinguishable from `Ok` after a write unless the caller can
/// ask why, and the deck's own reset gives no second chance to find out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeartbeatState {
    /// The deck does not say. Nothing is known, and a caller that logs this is
    /// reporting a gap rather than a fault.
    #[default]
    Unreported,
    /// A report is due: the next call writes one.
    Due,
    /// The deck was never woken. A sleeping deck is supposed to be idle, so
    /// this is silence by design — and the state that turns a heartbeat into a
    /// no-op nobody notices.
    Asleep,
    /// An image transfer is queued. A report now would land inside it and
    /// corrupt the upload.
    ImagePending,
    /// The deck was written to recently enough that no report is due.
    RecentWrite,
}

impl HeartbeatState {
    /// Whether this state means the deck is not being fed.
    ///
    /// `Due` and `RecentWrite` both describe a healthy heartbeat: one that is
    /// about to write and one that just did. Everything else is silence, and
    /// silence is what the deck punishes.
    ///
    /// ```
    /// use deck_api::HeartbeatState;
    ///
    /// assert!(HeartbeatState::Unreported.is_blocked());
    /// assert!(HeartbeatState::Asleep.is_blocked());
    /// assert!(HeartbeatState::ImagePending.is_blocked());
    /// assert!(!HeartbeatState::Due.is_blocked());
    /// ```
    pub fn is_blocked(self) -> bool {
        matches!(
            self,
            HeartbeatState::Unreported | HeartbeatState::Asleep | HeartbeatState::ImagePending
        )
    }
}

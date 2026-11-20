//! What the machine is playing, as Windows reports it.
//!
//! Windows keeps one **media session** per player — Spotify, a browser tab, the
//! system's own — and the same one that obeys the deck's media keys is the one
//! this reads. `GlobalSystemMediaTransportControlsSessionManager` is the API, it
//! is WinRT, and every call in it is `unsafe`: this module lives in the crate the
//! project allows that, and every status is checked before a value is used.
//!
//! ## What it answers
//!
//! The current session's title and artist, or `None` when nothing is playing and
//! the system has no session to name. A player that is open but silent usually
//! still has a session, so a title does not mean a sound.
//!
//! ## Cost
//!
//! Three WinRT objects per call, which is cheap next to a temperature read, but
//! not free: the plugin asks at its own cadence and nowhere near a frame.

#[cfg(windows)]
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

/// COM, entered on this thread the first time the session is asked about.
///
/// `RPC_E_CHANGED_MODE` is not a failure: the thread is already in another
/// apartment, and WinRT answers a session manager in either.
#[cfg(windows)]
fn enter_apartment() -> Option<()> {
    use std::cell::Cell;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    thread_local! {
        static APARTMENT: Cell<Option<bool>> = const { Cell::new(None) };
    }

    APARTMENT.with(|slot| match slot.get() {
        Some(entered) => entered.then_some(()),
        None => {
            // SAFETY: no reserved value is passed and the apartment model is the
            // one a WinRT session manager expects. The apartment outlives this
            // call on purpose: tearing COM down under a live object is a crash.
            let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let entered = result.is_ok() || result == RPC_E_CHANGED_MODE;
            slot.set(Some(entered));
            entered.then_some(())
        }
    })
}

/// What one player is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    /// The track's title, as the player states it.
    pub title: String,
    /// The artist, or an empty string when the player names none.
    pub artist: String,
}

/// The current media session's title and artist, when there is one.
///
/// `None` for every ordinary reason: no session at all, a session that refuses
/// its properties, a player that answers with an empty title. A key that shows
/// `None` keeps the face it had, which is the same rule the gauges follow.
#[cfg(not(windows))]
pub fn now_playing() -> Option<NowPlaying> {
    None
}

#[cfg(windows)]
/// The current media session's title and artist, when there is one.
///
/// `None` for every ordinary reason: no session at all, a session that refuses
/// its properties, a player that answers with an empty title. A key that shows
/// `None` keeps the face it had, which is the same rule the gauges follow.
pub fn now_playing() -> Option<NowPlaying> {
    // WinRT lives on a COM apartment. Win32 COM is entered here rather than
    // borrowed from another crate: this module owns its own initialisation, and
    // entering twice is answered rather than refused.
    enter_apartment()?;

    // SAFETY: every call here answers a `Result`, checked with `ok()?`, and the
    // objects it hands back are released by their own `Drop` before this returns.
    // `join` is windows-future's blocking wait for the operation.
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .ok()?
        .join()
        .ok()?;
    let session = manager.GetCurrentSession().ok()?;
    let properties = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;

    let title = properties.Title().ok()?.to_string();
    if title.trim().is_empty() {
        return None;
    }
    let artist = properties
        .Artist()
        .map(|artist| artist.to_string())
        .unwrap_or_default();

    Some(NowPlaying {
        title: title.trim().to_string(),
        artist: artist.trim().to_string(),
    })
}

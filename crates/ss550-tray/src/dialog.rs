//! The system's own "open a picture" dialog.
//!
//! The picture a user wants for a key is almost never in the folder the app
//! owns: it is in `Downloads`, on the Desktop, or somewhere inside an icon pack
//! with six thousand files in it. Listing that folder was the wrong shape for
//! the problem, so the window asks Windows where the file is instead, and
//! imports what comes back.
//!
//! ## What the dialog offers
//!
//! One filter, built from the engine's own
//! [`ACCEPTED_EXTENSIONS`](ss550_engine::assets::ACCEPTED_EXTENSIONS): the
//! formats the asset library accepts and nothing else. There is no "all files"
//! entry on purpose — a `.gif` reachable through one is a file the library
//! refuses a moment later, and a refusal after the fact is a worse answer than a
//! file that was never offered.
//!
//! ## Cost
//!
//! `GetOpenFileNameW` runs the dialog's own message loop on the calling thread
//! and returns when the user answers. It is therefore a click, never a frame:
//! the caller blocks while the dialog is up.
//!
//! ## Which thread opens it
//!
//! A thread that already owns a message loop — the one a window toolkit
//! dispatches on — must not be the one that calls this. The dialog pumps its own
//! loop, so messages arrive at the outer toolkit from inside a call the outer
//! loop made: a second, re-entrant dispatch on a thread that is already inside
//! one. GPUI answers that with a panic (the app is borrowed twice), and any
//! toolkit with the same invariant answers it the same way.
//!
//! So the window opens this on a worker thread and reads the answer back as
//! state. [`open_image_owned_by`] takes the window the dialog is modal to, so it
//! still disables the right window and centres on it.
//!
//! # Example
//!
//! ```no_run
//! // Opened on a worker thread: the dialog pumps its own message loop, so the
//! // thread that dispatches the window's messages must not be the one that
//! // calls it.
//! let (sender, answer) = std::sync::mpsc::channel();
//! std::thread::spawn(move || {
//!     let _ = sender.send(ss550_tray::dialog::open_image_owned_by(None));
//! });
//! if let Some(path) = answer.recv().ok().flatten() {
//!     println!("importing {}", path.display());
//! }
//! ```

use std::path::PathBuf;

use ss550_engine::assets::ACCEPTED_EXTENSIONS;

/// The title the dialog is shown with.
///
/// A title, not an instruction: the dialog already says what it wants, and the
/// window's own sentence about importing is on the card the click came from.
pub const TITLE: &str = "Choose a picture";

/// The label of the one filter the dialog offers.
pub const FILTER_LABEL: &str = "Pictures";

/// Asks the user for a picture file, with the system's own dialog.
///
/// The dialog is filtered to the formats the asset library accepts — see
/// [`patterns`] — and opens in the user's Pictures folder.
///
/// # Returns
///
/// `Some(path)` once a file is picked and confirmed. `None` when the dialog
/// answers "cancel", and also when it could not be shown at all: a thread with
/// no message loop, a shell that refuses to answer. The two are not told apart,
/// because the caller has nothing different to do about them — either way it has
/// no file to import, and the only other thing it could say is a sentence about
/// a dialog the user never saw. A debug build writes the common-dialog error
/// code to stderr when the dialog failed, so the difference is not lost while
/// developing; a release build has no console to write it to.
///
/// # Blocking
///
/// This does not return until the user answers: it runs the dialog's own modal
/// message loop on the calling thread. A window that owns an event loop must
/// therefore call it from a worker thread — see the module documentation for
/// why — and read the answer back as state. [`open_image_owned_by`] is the same
/// call with a window for the dialog to be modal to.
pub fn open_image() -> Option<PathBuf> {
    open_image_owned_by(None)
}

/// [`open_image`], with the window the dialog belongs to.
///
/// `owner` is the native handle of the window the dialog is modal to: Windows
/// disables it while the dialog is up and puts the dialog in front of it, which
/// is what stops the user from painting a key while they are choosing the
/// picture for it. `None` behaves exactly like [`open_image`] — the dialog
/// stands on its own, which is the right answer for a caller with no window.
///
/// The handle is an `isize`, the same currency [`crate::hide_window`] and
/// [`crate::show_window`] take, so a caller needs no platform handle type to ask
/// for a dialog. A handle that no longer names a window is not a failure: the
/// dialog opens without an owner.
///
/// # Example
///
/// ```no_run
/// # let hwnd = 0isize;
/// // On a worker thread; the answer comes back as state.
/// let path = ss550_tray::dialog::open_image_owned_by(Some(hwnd));
/// # let _ = path;
/// ```
pub fn open_image_owned_by(owner: Option<isize>) -> Option<PathBuf> {
    platform::open_image(owner)
}

/// The file patterns the dialog accepts, as one filter entry spells them.
///
/// Built from the engine's [`ACCEPTED_EXTENSIONS`], so the dialog and the
/// library cannot disagree about what a picture is: a format added to the
/// library is offered by the dialog the same day, and one the library refuses is
/// never offered.
pub fn patterns() -> String {
    ACCEPTED_EXTENSIONS
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<String>>()
        .join(";")
}

/// The filter block a common dialog takes, as nul-terminated UTF-16.
///
/// The shape is Windows', not this crate's: a label, its patterns, and then a
/// second nul, which is what ends the list. A block that stops after the first
/// pair makes the dialog read past it looking for the end.
pub fn filter() -> Vec<u16> {
    let patterns = patterns();
    let mut units = Vec::new();
    for text in [FILTER_LABEL, patterns.as_str()] {
        units.extend(text.encode_utf16());
        units.push(0);
    }
    units.push(0);
    units
}

/// Everything that needs Win32.
#[cfg(windows)]
mod platform {
    use std::ffi::{c_void, OsString};
    use std::mem::size_of;
    use std::os::windows::ffi::OsStringExt as _;
    use std::path::PathBuf;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, COMMON_DLG_ERRORS, OFN_EXPLORER, OFN_FILEMUSTEXIST,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    use super::{filter, TITLE};

    /// How many UTF-16 units the chosen path is read into.
    ///
    /// Well past `MAX_PATH`, because a picture inside an unpacked icon pack sits
    /// several folders deep and the path to it is longer than 260 characters. A
    /// path that does not fit is truncated by the dialog rather than overrunning
    /// the buffer.
    const PATH_UNITS: usize = 1024;

    /// Opens the dialog and reads the path it answered with.
    pub(super) fn open_image(owner: Option<isize>) -> Option<PathBuf> {
        // The Pictures folder is a known folder, and the shell answers those
        // through COM. A thread that cannot enter an apartment still gets the
        // dialog — it just opens in the working directory.
        let initial = pictures_folder();

        let filter = filter();
        let title = wide(TITLE);
        let mut buffer = vec![0u16; PATH_UNITS];

        let mut dialog = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            // The window the dialog is modal to. It may belong to another
            // thread — that is how a dialog is opened from a worker without
            // giving up the window it belongs to — and a null handle is the
            // documented "no owner", which is what `None` means here.
            hwndOwner: match owner {
                Some(hwnd) => HWND(hwnd as *mut c_void),
                None => HWND::default(),
            },
            lpstrFilter: PCWSTR(filter.as_ptr()),
            // One-based: the first entry of the filter block, which is the only
            // one there is.
            nFilterIndex: 1,
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: PATH_UNITS as u32,
            lpstrInitialDir: match &initial {
                Some(folder) => PCWSTR(folder.as_ptr()),
                None => PCWSTR::null(),
            },
            lpstrTitle: PCWSTR(title.as_ptr()),
            // `OFN_EXPLORER` is what keeps the dialog from changing the
            // process's working directory, which the rest of the window would
            // then resolve its own paths against. The other two make the answer
            // a file that exists: a name typed by hand into the box is still a
            // path the library has to be able to open.
            Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
            ..Default::default()
        };

        // SAFETY: every pointer in `dialog` points at a live buffer owned by
        // this function, `lStructSize` is the size of the record the call is
        // given, and `nMaxFile` is the length of the path buffer. The call fills
        // at most `nMaxFile` units and answers with a `BOOL`; nothing is read out
        // of the buffer unless it answered true.
        let answered = unsafe { GetOpenFileNameW(&mut dialog) };
        if !answered.as_bool() {
            note_failure();
            return None;
        }

        // The buffer holds a nul-terminated path; everything past the first nul
        // is whatever was there before, which is nothing.
        let length = buffer.iter().position(|unit| *unit == 0)?;
        Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
    }

    /// Writes the dialog's own error code to stderr, in a build that has one.
    ///
    /// A zero code is the user pressing "cancel", which is not a failure and
    /// says nothing. Anything else is the dialog refusing to open, and that is
    /// worth a line while developing — there is no console in a release build,
    /// and no window to put it in front of the user.
    fn note_failure() {
        // SAFETY: no arguments, and the call only reads a thread-local the
        // common dialog module keeps.
        let code = unsafe { CommDlgExtendedError() };
        if code == COMMON_DLG_ERRORS::default() {
            return;
        }
        #[cfg(debug_assertions)]
        eprintln!("the picture dialog did not open: {code:?}");
    }

    /// The user's Pictures folder, as a nul-terminated UTF-16 path.
    ///
    /// `None` when the shell will not say where it is, which is not a failure:
    /// the dialog then opens in the process's working directory, which is what
    /// Windows does with no starting folder.
    fn pictures_folder() -> Option<Vec<u16>> {
        crate::shortcut::enter_apartment()?;

        // SAFETY: the folder id is a constant the shell owns, the flags ask for
        // the default path of the current user, and the token is "this process".
        // The call answers a pointer the shell allocated, or an error.
        let raw =
            unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) }.ok()?;

        // SAFETY: the call answered a nul-terminated string it allocated, which
        // is read here and freed below, exactly once.
        let path = unsafe { raw.as_wide() };
        let mut units: Vec<u16> = path.to_vec();
        // SAFETY: the pointer came from `SHGetKnownFolderPath`, which allocates
        // with the COM task allocator, and it is freed once and never used again.
        unsafe { CoTaskMemFree(Some(raw.0 as *const c_void)) };

        units.push(0);
        Some(units)
    }

    /// `text` as a nul-terminated UTF-16 string.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

/// A file dialog is Windows behaviour; elsewhere there is nothing to ask.
#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;

    /// No dialog on this platform in this delivery.
    ///
    /// The window keeps working: a key with no picture shows its colour, and the
    /// button that would open a dialog answers "no file was chosen".
    pub(super) fn open_image(_owner: Option<isize>) -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{filter, patterns, FILTER_LABEL, TITLE};
    use ss550_engine::assets::ACCEPTED_EXTENSIONS;

    /// The filter block as its parts: the strings between the nuls.
    fn entries(units: &[u16]) -> Vec<String> {
        units
            .split(|unit| *unit == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf16(part).expect("the filter is UTF-16"))
            .collect()
    }

    #[test]
    fn the_dialog_offers_exactly_the_formats_the_library_accepts() {
        // One filter entry, one label, one list of patterns: the dialog cannot
        // offer a format the library refuses, and cannot hide one it takes.
        let patterns = patterns();
        let offered: Vec<&str> = patterns.split(';').collect();
        let expected: Vec<String> = ACCEPTED_EXTENSIONS
            .iter()
            .map(|extension| format!("*.{extension}"))
            .collect();

        assert_eq!(offered, expected);
        for extension in ACCEPTED_EXTENSIONS {
            assert!(
                offered.contains(&format!("*.{extension}").as_str()),
                "{extension} is accepted by the library and missing from the dialog"
            );
        }
    }

    #[test]
    fn the_filter_is_a_label_its_patterns_and_a_terminator() {
        let block = filter();

        assert_eq!(
            entries(&block),
            [FILTER_LABEL.to_string(), patterns()],
            "one entry: the label, then the patterns it covers"
        );
        assert_eq!(
            block.last(),
            Some(&0),
            "the block ends with a nul, and the empty part before it is the second one"
        );
        assert_eq!(
            block.iter().rev().take(2).copied().collect::<Vec<u16>>(),
            [0, 0],
            "a filter list is ended by two nuls, not one"
        );
    }

    #[test]
    fn every_pattern_is_a_wildcard_with_no_directory_in_it() {
        // The dialog matches these against file names. A pattern carrying a path
        // separator would match nothing at all, which is a filter that silently
        // shows an empty folder.
        for pattern in patterns().split(';') {
            assert!(pattern.starts_with("*."), "{pattern}");
            assert!(!pattern.contains(['/', '\\']), "{pattern}");
        }
    }

    #[test]
    fn the_dialog_has_words_of_its_own() {
        // The title is what the user reads in the title bar; an empty one makes
        // the dialog look like a system error rather than a choice.
        assert!(!TITLE.is_empty());
        assert!(!FILTER_LABEL.is_empty());
    }
}

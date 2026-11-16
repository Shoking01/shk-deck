//! The applications installed on this PC, as the `RUN APP` editor offers them.
//!
//! The list comes from [`ss550_tray::installed_apps`], which walks the Start Menu
//! and resolves every shortcut it finds. This module adds the one thing the tray
//! crate does not do — the picture each entry is drawn with — and the state the
//! picker needs while it is being scanned.
//!
//! ## When the work happens
//!
//! [`scan`] is the only function here that reads the filesystem or asks the
//! shell for anything, and it is called from a background task started by the
//! click that opens the picker. The result comes back as [`Apps::Ready`] and is
//! kept: a second click on CHOOSE AN APP lists the same applications without
//! touching the disk. Nothing in this module is ever called from `render`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::RenderImage;
use image::RgbaImage;

use crate::icons;
use crate::render;

/// The side a picker row's icon is shrunk to, in pixels.
///
/// The row draws it at 44 pixels; a couple of spare pixels keep the picture from
/// being resampled by the GPU on a display that scales it up a little.
const ROW_ICON_SIDE: u32 = 48;

/// One application the picker can put in `PROGRAM`.
pub(crate) struct AppChoice {
    /// What the list calls it: the shortcut's stem, with no `.lnk`.
    pub(crate) name: String,
    /// What goes in the field. The shortcut when there is one, so the runner
    /// hands it to the shell exactly as Explorer would.
    pub(crate) program: PathBuf,
    /// The program behind it, shown under the name so two entries that look
    /// alike can be told apart.
    pub(crate) target: PathBuf,
    /// The application's own icon, read once at scan time.
    pub(crate) icon: Option<Arc<RenderImage>>,
}

impl AppChoice {
    /// Whether `query` — already lower-cased — names this application.
    ///
    /// The name is what the user is looking for; the target is there because a
    /// person who knows the program's folder will type part of that instead.
    pub(crate) fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        self.name.to_lowercase().contains(query)
            || self.target.to_string_lossy().to_lowercase().contains(query)
    }
}

/// What the picker knows about this machine.
pub(crate) enum Apps {
    /// Nothing has been scanned yet. The list would be a lie, and the scan is
    /// the only slow step, so it waits for a click that asks for it.
    Unscanned,
    /// A scan is in flight.
    Scanning,
    /// The applications that were found, or an empty list when there are none.
    Ready(Vec<AppChoice>),
}

impl Apps {
    /// Whether a scan has already run — an empty answer included.
    ///
    /// A machine with nothing in its Start Menu has been scanned, and asking
    /// again would only make the user wait for the same empty list.
    pub(crate) fn is_scanned(&self) -> bool {
        matches!(self, Apps::Ready(_))
    }

    /// The applications, when the scan has run.
    pub(crate) fn list(&self) -> Option<&[AppChoice]> {
        match self {
            Apps::Ready(apps) => Some(apps),
            _ => None,
        }
    }
}

/// Walks the Start Menu and reads one icon per application.
///
/// Slow on purpose: hundreds of shortcuts, one COM object and one shell icon
/// each. It runs on the background executor — see [`Apps`] — and the answer is
/// plain data that the window keeps.
pub(crate) fn scan() -> Vec<AppChoice> {
    ss550_tray::installed_apps()
        .into_iter()
        .map(|app| AppChoice {
            name: app.name().to_string(),
            program: app.shortcut().to_path_buf(),
            target: app.target().to_path_buf(),
            // The program, not the shortcut: the scan already resolved the link,
            // and asking about the shortcut again would be a second COM object
            // per application for the same picture.
            icon: icon_for(app.target()),
        })
        .collect()
}

/// The icon of one program, as the renderer wants it.
///
/// An icon with no pixels is not a picture: the row would draw a black square
/// where the name alone reads better, so it is a miss and the picker draws the
/// initial instead.
fn icon_for(program: &Path) -> Option<Arc<RenderImage>> {
    let pixels = ss550_tray::appicon::program_icon(program)?;
    if pixels.is_blank() {
        return None;
    }
    let icon = RgbaImage::from_raw(pixels.width(), pixels.height(), pixels.rgba().to_vec())?;
    // A list row is 44 pixels across, and the shell answers with the jumbo
    // 256-pixel icon: holding one of those per application would cost a quarter
    // of a megabyte each for a thumbnail, on a list of hundreds. It is shrunk
    // once, here, on the scan's background thread — the row draws the small one
    // and never resamples anything.
    let thumbnail = icons::shrink_icon(&icon, ROW_ICON_SIDE);
    Some(Arc::new(RenderImage::new([render::frame_from_rgba(
        &thumbnail,
    )])))
}

#[cfg(test)]
mod tests {
    use super::AppChoice;
    use std::path::PathBuf;

    fn choice(name: &str, target: &str) -> AppChoice {
        AppChoice {
            name: name.to_string(),
            program: PathBuf::from(format!("C:\\Start Menu\\{name}.lnk")),
            target: PathBuf::from(target),
            icon: None,
        }
    }

    #[test]
    fn a_query_matches_the_name_without_regard_to_case() {
        let brave = choice("Brave", r"C:\Program Files\Brave\brave.exe");

        assert!(brave.matches(""));
        assert!(brave.matches("bra"));
        assert!(brave.matches("brave"));
        assert!(!brave.matches("obsidian"));
    }

    #[test]
    fn a_query_also_matches_the_folder_the_program_lives_in() {
        // Someone who knows where a program is installed types that instead of
        // its Start Menu name.
        let brave = choice("Brave", r"C:\Program Files\Brave\brave.exe");

        assert!(brave.matches("program files"));
        assert!(brave.matches("brave.exe"));
    }

    #[test]
    fn an_unscanned_picker_is_not_the_same_as_an_empty_one() {
        use super::Apps;

        assert!(!Apps::Unscanned.is_scanned());
        assert!(!Apps::Scanning.is_scanned());
        assert!(
            Apps::Ready(Vec::new()).is_scanned(),
            "an empty answer was still an answer"
        );
        assert!(Apps::Ready(Vec::new()).list().is_some());
        assert!(Apps::Unscanned.list().is_none());
    }
}

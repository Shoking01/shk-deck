//! The window's flavours: the named palettes the whole program is painted in.
//!
//! A flavour is a gpui-kit theme file, embedded in the binary. That is what
//! makes it reach everything: the library's own controls (buttons, inputs,
//! switches, scrollbars) read the theme, and this crate's chrome reads the same
//! theme through [`crate::ui::Tokens`], so one choice repaints both. A palette
//! written only in Rust would leave the hosted controls on the old colours.
//!
//! The design's own palette is the first flavour and the default, so a fresh
//! install looks the way the window was designed to look; the others are there
//! because a desk is not one taste.
//!
//! The choice is one line in `config_dir()/flavour.txt`, beside the other
//! per-user settings. It is read at start, before the first frame, so a window
//! never flashes the default palette on its way to the chosen one.

use gpui_kit::component::theme::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::App;
use std::path::Path;

/// Where the chosen flavour is kept.
const FLAVOUR_FILE: &str = "flavour.txt";

/// One named palette.
///
/// The accent is restated here rather than read back out of the theme: the
/// picker draws its swatch from it, and the deck's key glyphs are painted in it,
/// both of which need the colour before any window exists.
pub(crate) struct Flavour {
    /// The name the theme file and the picker use.
    pub(crate) name: &'static str,
    /// What it looks like, in the picker's own words.
    pub(crate) note: &'static str,
    /// The accent as `0xRRGGBB`, for the swatch and the deck's glyphs.
    pub(crate) accent: u32,
    /// The mode the theme file declares.
    pub(crate) mode: ThemeMode,
    /// The theme set carrying exactly this flavour.
    file: &'static str,
}

/// The flavours this build ships, in picker order.
pub(crate) const FLAVOURS: &[Flavour] = &[
    Flavour {
        name: "Sh-Deck Ink",
        note: "The design's own violet, on near-black",
        accent: 0x7c6cf0,
        mode: ThemeMode::Dark,
        file: include_str!("../assets/themes/ink.json"),
    },
    Flavour {
        name: "Catppuccin Mocha",
        note: "Mauve on a warm midnight",
        accent: 0xcba6f7,
        mode: ThemeMode::Dark,
        file: include_str!("../assets/themes/catppuccin-mocha.json"),
    },
    Flavour {
        name: "Catppuccin Latte",
        note: "The same flavours, poured light",
        accent: 0x8839ef,
        mode: ThemeMode::Light,
        file: include_str!("../assets/themes/catppuccin-latte.json"),
    },
    Flavour {
        name: "One Dark",
        note: "Atom's editor, purple on graphite",
        accent: 0xc678dd,
        mode: ThemeMode::Dark,
        file: include_str!("../assets/themes/one-dark.json"),
    },
];

impl Flavour {
    /// The accent as the deck's pictures take it: three bytes, no alpha.
    pub(crate) fn accent_rgb(&self) -> [u8; 3] {
        [
            (self.accent >> 16) as u8,
            (self.accent >> 8) as u8,
            self.accent as u8,
        ]
    }

    /// Installs this flavour in the running app.
    ///
    /// The theme file is registered once and then installed through
    /// [`Theme::apply_config`], which is the library's own "this file is the
    /// palette now" path: it replaces the colours, the radius and the highlight
    /// theme together. The mode is set beside it because a flavour carries its
    /// own — Latte is a light palette, and asking for it must turn the window
    /// light rather than paint light colours in dark mode.
    pub(crate) fn apply(&self, cx: &mut App) {
        // Registering is idempotent: the registry keeps the first file that
        // claimed a name, so a second apply finds the theme it already has.
        let _ = ThemeRegistry::global_mut(cx).load_themes_from_str(self.file);
        let Some(config) = ThemeRegistry::global(cx).themes().get(self.name).cloned() else {
            // The file did not parse. Staying on the palette already in force is
            // the honest answer: a window that silently becomes the library's
            // default grey would look like a different program.
            return;
        };
        Theme::update(cx, |theme| {
            theme.apply_config(&config);
            theme.mode = config.mode;
        });
    }
}

/// The flavour the window starts in: the saved one, or the design's own.
///
/// A name that no longer exists (a flavour dropped from a build, a file edited
/// by hand) falls back to the default rather than refusing to start.
pub(crate) fn load() -> &'static Flavour {
    load_at(&ss550_engine::config_dir())
}

/// [`load`], against a directory a test owns.
pub(crate) fn load_at(dir: &Path) -> &'static Flavour {
    let Ok(text) = std::fs::read_to_string(dir.join(FLAVOUR_FILE)) else {
        return default();
    };
    let wanted = text.trim();
    FLAVOURS
        .iter()
        .find(|flavour| flavour.name == wanted)
        .unwrap_or_else(|| default())
}

/// The design's own palette, which is what an install starts with.
pub(crate) fn default() -> &'static Flavour {
    // The list is a constant with at least one entry; the index cannot miss.
    &FLAVOURS[0]
}

/// Writes the choice where the next start reads it.
pub(crate) fn save(name: &str) -> Result<(), String> {
    save_at(&ss550_engine::config_dir(), name)
}

/// [`save`], against a directory a test owns.
pub(crate) fn save_at(dir: &Path, name: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let path = dir.join(FLAVOUR_FILE);
    std::fs::write(&path, format!("{name}\n"))
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{default, load_at, save_at, FLAVOURS};

    /// A scratch directory of this test's own, removed when it is dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("ss550-flavour-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("a scratch directory");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_design_s_own_palette_is_the_default() {
        let scratch = Scratch::new("default");

        assert_eq!(load_at(&scratch.0).name, default().name);
        assert_eq!(default().name, FLAVOURS[0].name);
    }

    #[test]
    fn a_saved_flavour_is_the_one_the_next_start_reads() {
        let scratch = Scratch::new("roundtrip");
        let chosen = &FLAVOURS[1];

        save_at(&scratch.0, chosen.name).expect("a writable scratch directory");

        assert_eq!(load_at(&scratch.0).name, chosen.name);
    }

    #[test]
    fn a_flavour_this_build_does_not_ship_falls_back_instead_of_refusing() {
        let scratch = Scratch::new("unknown");

        save_at(&scratch.0, "Solarized Walnut").expect("a writable scratch directory");

        assert_eq!(
            load_at(&scratch.0).name,
            default().name,
            "an unknown name must not leave the window without a palette"
        );
    }

    #[test]
    fn every_flavour_states_its_accent_in_the_mode_its_file_declares() {
        // The swatch and the deck's glyphs are painted from the accent in this
        // table, and the window is painted from the file. A pair that disagreed
        // on the mode would paint a light palette in dark mode, which is the one
        // combination no theme file can rescue.
        for flavour in FLAVOURS {
            assert!(
                flavour
                    .file
                    .contains(&format!("\"name\": \"{}\"", flavour.name)),
                "{} carries a theme file that names something else",
                flavour.name
            );
            let declared = if flavour.file.contains("\"mode\": \"dark\"") {
                "dark"
            } else {
                "light"
            };
            assert_eq!(
                flavour.mode.name(),
                declared,
                "{} declares one mode and states another",
                flavour.name
            );
        }
    }

    #[test]
    fn no_two_flavours_share_a_name() {
        // The registry keys themes by name, so a duplicate would silently
        // replace the first one in the picker.
        let mut names: Vec<&str> = FLAVOURS.iter().map(|flavour| flavour.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();

        assert_eq!(names.len(), before);
    }
}

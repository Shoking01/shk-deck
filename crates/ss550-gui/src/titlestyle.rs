//! How every title looks: its type size, where it sits, and which bitmap face
//! draws it.
//!
//! A title is drawn twice — into the deck's JPEG and over the window's tile — so
//! the choice has to live somewhere both sides read. It is app-wide, one style
//! for every deck and every key, kept as JSON in
//! `config_dir()/title-style.json`, and applied by repainting the keys that
//! carry a title through the same path a title change uses. Nothing here
//! measures or draws text: the layout runs when a payload is composed, never
//! while a frame is being painted.
//!
//! ## Why the file is read by hand
//!
//! The engine keeps its JSON behind `ss550_engine::config::read_json`, which is
//! crate-private, and it has no reader for a file of three words. The window
//! therefore writes and reads this one file itself: a BOM strip and three
//! quoted values, no parser dependency, and a missing or unreadable file falls
//! back to the default instead of failing the start.

use std::path::PathBuf;

/// The file the style lives in, inside the app's configuration folder.
pub(crate) const TITLE_STYLE_FILE: &str = "title-style.json";

/// The window's own label size, in pixels.
///
/// A title in the window is a micro-label, the same step every other caption
/// uses; the style scales it. The `ui` test pins this to `FONT_MICRO`, so the
/// two cannot drift apart.
pub(crate) const WINDOW_BASE: f32 = 11.0;

/// How large a title's type may be drawn.
///
/// The size is a cap on the whole-pixel scale of the bitmap, not a font: the
/// layout still steps down until the words fit the tile, so a size can only
/// make type smaller than the automatic fit, never larger than the tile allows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum TitleSize {
    /// At most two dots a pixel: a long title fits with room to spare.
    Small,
    /// The automatic fit — the largest whole scale the tile holds.
    #[default]
    Medium,
    /// One step more than the fit, by letting the words use more of the tile.
    Large,
}

impl TitleSize {
    /// Every size, in the order the settings page shows them.
    pub(crate) const ALL: [TitleSize; 3] = [TitleSize::Small, TitleSize::Medium, TitleSize::Large];

    /// The word this size is written as, in the file and in the URL of a
    /// setting.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TitleSize::Small => "small",
            TitleSize::Medium => "medium",
            TitleSize::Large => "large",
        }
    }

    /// The word the settings page prints.
    pub(crate) fn label(self) -> &'static str {
        match self {
            TitleSize::Small => "SMALL",
            TitleSize::Medium => "MEDIUM",
            TitleSize::Large => "LARGE",
        }
    }

    /// Reads the word back, case-insensitively.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "small" => Some(TitleSize::Small),
            "medium" => Some(TitleSize::Medium),
            "large" => Some(TitleSize::Large),
            _ => None,
        }
    }

    /// The most dots one dot of the bitmap may be drawn at.
    ///
    /// `u32::MAX` is "no cap": the fit decides. Small's cap is the size a long
    /// title already lands on, so it never has to drop a word to get smaller.
    pub(crate) fn scale_cap(self) -> u32 {
        match self {
            TitleSize::Small => 2,
            TitleSize::Medium | TitleSize::Large => u32::MAX,
        }
    }

    /// How much of the tile the words may use, given the position's own share.
    ///
    /// Small shares the box with Medium: three lines at one dot a pixel already
    /// fill a band, so a shorter box would cut words instead of shrinking them.
    /// Large is the one size that takes more of the tile, which is what gives
    /// the fit headroom to step up.
    pub(crate) fn box_percent(self, base: u32) -> u32 {
        match self {
            TitleSize::Small | TitleSize::Medium => base,
            TitleSize::Large => (base * 3 / 2).min(92),
        }
    }
}

/// Where on the tile a title sits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum TitlePosition {
    /// A band along the top edge, over a picture.
    Top,
    /// A block in the middle, on the words themselves.
    Middle,
    /// A band along the bottom edge — the caption a picture usually carries.
    #[default]
    Bottom,
}

impl TitlePosition {
    /// Every position, in the order the settings page shows them.
    pub(crate) const ALL: [TitlePosition; 3] = [
        TitlePosition::Top,
        TitlePosition::Middle,
        TitlePosition::Bottom,
    ];

    /// The word this position is written as, in the file and in a setting id.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TitlePosition::Top => "top",
            TitlePosition::Middle => "middle",
            TitlePosition::Bottom => "bottom",
        }
    }

    /// The word the settings page prints.
    pub(crate) fn label(self) -> &'static str {
        match self {
            TitlePosition::Top => "TOP",
            TitlePosition::Middle => "MIDDLE",
            TitlePosition::Bottom => "BOTTOM",
        }
    }

    /// Reads the word back, case-insensitively.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "top" => Some(TitlePosition::Top),
            "middle" => Some(TitlePosition::Middle),
            "bottom" => Some(TitlePosition::Bottom),
            _ => None,
        }
    }
}

/// Which bitmap face draws a title.
///
/// A face is a table plus its dot dimensions, not a second renderer: both go
/// through the same layout and the same drawing loop. Compact is three dots
/// wide and five tall, so the same words take less of the tile and more of them
/// fit on a line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum TitleFace {
    /// Five dots wide, seven tall: the face the initial of a key already uses.
    #[default]
    Standard,
    /// Three dots wide, five tall: a smaller face that fits more characters.
    Compact,
}

impl TitleFace {
    /// Every face, in the order the settings page shows them.
    pub(crate) const ALL: [TitleFace; 2] = [TitleFace::Standard, TitleFace::Compact];

    /// The word this face is written as, in the file and in a setting id.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TitleFace::Standard => "standard",
            TitleFace::Compact => "compact",
        }
    }

    /// The word the settings page prints.
    pub(crate) fn label(self) -> &'static str {
        match self {
            TitleFace::Standard => "STANDARD",
            TitleFace::Compact => "COMPACT",
        }
    }

    /// Reads the word back, case-insensitively.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "standard" => Some(TitleFace::Standard),
            "compact" => Some(TitleFace::Compact),
            _ => None,
        }
    }

    /// The dots one character of this face occupies, `(width, height)`.
    ///
    /// This is the whole of the difference between the two faces as far as the
    /// layout is concerned: one advance and one line height follow from it.
    pub(crate) fn dots(self) -> (u32, u32) {
        match self {
            TitleFace::Standard => (5, 7),
            TitleFace::Compact => (3, 5),
        }
    }
}

/// One title style, applied to every titled key.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct TitleStyle {
    /// How large the type may be drawn.
    pub size: TitleSize,
    /// Where on the tile the words sit.
    pub position: TitlePosition,
    /// Which bitmap face draws them.
    pub face: TitleFace,
}

impl TitleStyle {
    /// The style as the settings page and the status line describe it.
    pub(crate) fn summary(self) -> String {
        let place = match self.position {
            TitlePosition::Top => "at the top",
            TitlePosition::Middle => "in the middle",
            TitlePosition::Bottom => "at the bottom",
        };
        format!(
            "{} type, {place}, {} face",
            self.size.as_str(),
            self.face.as_str()
        )
    }

    /// The type size the window draws a title at, on a tile `side` px tall.
    ///
    /// A pure function of the style and the tile, so no frame measures a string:
    /// the window's label is the same step as every other caption, scaled by the
    /// style and clamped to the tile it sits on. The deck's bitmap is fitted to
    /// its own box separately; the two agree about the style, not about a font.
    pub(crate) fn window_font(self, side: f32) -> f32 {
        let size = match self.size {
            TitleSize::Small => 0.85,
            TitleSize::Medium => 1.0,
            TitleSize::Large => 1.2,
        };
        // The compact face is smaller by design; its window label steps down
        // with it, so the two faces do not read as the same size.
        let face = match self.face {
            TitleFace::Standard => 1.0,
            TitleFace::Compact => 0.85,
        };
        (WINDOW_BASE * size * face).clamp(7.0, (side * 0.28).max(7.0))
    }

    /// The style as the bytes of the file.
    fn to_json(self) -> String {
        format!(
            "{{\n  \"version\": 1,\n  \"size\": \"{}\",\n  \"position\": \"{}\",\n  \"face\": \"{}\"\n}}\n",
            self.size.as_str(),
            self.position.as_str(),
            self.face.as_str()
        )
    }

    /// Reads a style out of `text`, and says whether any field was understood.
    ///
    /// Every field falls back on its own, so a file that lost one line keeps the
    /// other two. The flag is what tells [`load`] the difference between a file
    /// this build wrote and something that is not a title style at all.
    fn from_json(text: &str) -> (Self, bool) {
        // A BOM is legal UTF-8 and every editor that offers "UTF-8" in older
        // Windows tooling writes one. The engine's reader is crate-private, so
        // the one comparison is repeated here rather than widening its API.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut style = TitleStyle::default();
        let mut understood = false;

        if let Some(value) = quoted_field(text, "size") {
            if let Some(size) = TitleSize::parse(value) {
                style.size = size;
                understood = true;
            }
        }
        if let Some(value) = quoted_field(text, "position") {
            if let Some(position) = TitlePosition::parse(value) {
                style.position = position;
                understood = true;
            }
        }
        if let Some(value) = quoted_field(text, "face") {
            if let Some(face) = TitleFace::parse(value) {
                style.face = face;
                understood = true;
            }
        }

        (style, understood)
    }
}

/// Where the style is kept.
pub(crate) fn path() -> PathBuf {
    ss550_engine::config_dir().join(TITLE_STYLE_FILE)
}

/// Reads the saved style, or the default when there is nothing usable.
///
/// The second half is a sentence for the status line. A file that was never
/// written is a first run, not a fault — the default is the answer and there is
/// nothing to report. A file that exists and cannot be read is a fault: the
/// user's choice was lost, and the sentence says so.
pub(crate) fn load() -> (TitleStyle, Option<String>) {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let (style, understood) = TitleStyle::from_json(&text);
            let note = (!understood).then(|| {
                format!(
                    "The title style in {} is not readable: using the default.",
                    path.display()
                )
            });
            (style, note)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (TitleStyle::default(), None),
        Err(error) => (
            TitleStyle::default(),
            Some(format!(
                "The title style could not be read ({error}): using the default."
            )),
        ),
    }
}

/// Writes the style where the next start reads it.
pub(crate) fn save(style: &TitleStyle) -> Result<(), String> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    std::fs::write(&path, style.to_json())
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// The string value of `key` in a flat JSON object, if it is there.
///
/// Deliberately not a parser: the file has three quoted words this build wrote,
/// and anything else is a reason to fall back to the default, not to fail. It
/// cannot panic — every step is an `Option`.
fn quoted_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let after_key = text.find(&needle)? + needle.len();
    let rest = text[after_key..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::{TitleFace, TitlePosition, TitleSize, TitleStyle, TITLE_STYLE_FILE};

    #[test]
    fn the_default_is_the_look_the_window_already_had() {
        let style = TitleStyle::default();
        assert_eq!(style.size, TitleSize::Medium);
        assert_eq!(style.position, TitlePosition::Bottom);
        assert_eq!(style.face, TitleFace::Standard);
    }

    #[test]
    fn every_size_reads_back_from_its_own_word() {
        for size in TitleSize::ALL {
            assert_eq!(TitleSize::parse(size.as_str()), Some(size));
            // The file is written by hand sometimes: case must not matter.
            assert_eq!(TitleSize::parse(&size.as_str().to_uppercase()), Some(size));
        }
        assert_eq!(TitleSize::parse("huge"), None);
        assert_eq!(TitleSize::parse(""), None);
    }

    #[test]
    fn every_position_reads_back_from_its_own_word() {
        for position in TitlePosition::ALL {
            assert_eq!(TitlePosition::parse(position.as_str()), Some(position));
            assert_eq!(
                TitlePosition::parse(&position.as_str().to_uppercase()),
                Some(position)
            );
        }
        assert_eq!(TitlePosition::parse("centre"), None);
    }

    #[test]
    fn every_face_reads_back_from_its_own_word() {
        for face in TitleFace::ALL {
            assert_eq!(TitleFace::parse(face.as_str()), Some(face));
            assert_eq!(TitleFace::parse(&face.as_str().to_uppercase()), Some(face));
        }
        assert_eq!(TitleFace::parse("bold"), None);
    }

    #[test]
    fn the_two_faces_are_a_table_and_its_dimensions() {
        assert_eq!(TitleFace::Standard.dots(), (5, 7));
        assert_eq!(TitleFace::Compact.dots(), (3, 5));
    }

    #[test]
    fn a_written_style_reads_back_field_for_field() {
        let style = TitleStyle {
            size: TitleSize::Large,
            position: TitlePosition::Top,
            face: TitleFace::Compact,
        };
        let (read, understood) = TitleStyle::from_json(&style.to_json());
        assert!(understood);
        assert_eq!(read, style);
    }

    #[test]
    fn a_bom_and_unknown_fields_do_not_hide_the_style() {
        let text = "\u{feff}{ \"version\": 1, \"face\": \"compact\", \"size\": \"small\", \
                    \"position\": \"top\", \"extra\": \"ignored\" }";
        let (style, understood) = TitleStyle::from_json(text);
        assert!(understood);
        assert_eq!(style.size, TitleSize::Small);
        assert_eq!(style.position, TitlePosition::Top);
        assert_eq!(style.face, TitleFace::Compact);
    }

    #[test]
    fn one_lost_field_keeps_the_other_two() {
        let text = r#"{ "size": "large", "face": "compact" }"#;
        let (style, understood) = TitleStyle::from_json(text);
        assert!(understood);
        assert_eq!(style.size, TitleSize::Large);
        assert_eq!(style.face, TitleFace::Compact);
        assert_eq!(
            style.position,
            TitlePosition::Bottom,
            "the default fills in"
        );
    }

    #[test]
    fn a_file_that_is_not_a_title_style_is_the_default_and_nothing_understood() {
        for text in ["", "not json", "{}", "{\"size\": \"enormous\"}"] {
            let (style, understood) = TitleStyle::from_json(text);
            assert!(!understood, "{text:?} should not read as a style");
            assert_eq!(style, TitleStyle::default(), "{text:?}");
        }
    }

    #[test]
    fn the_file_has_a_name_of_its_own_in_the_configuration_folder() {
        assert_eq!(TITLE_STYLE_FILE, "title-style.json");
        assert!(super::path().ends_with(TITLE_STYLE_FILE));
    }
}

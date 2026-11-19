//! How one key's title is drawn.
//!
//! A title is the short text a key shows, and this is the other half of it: the
//! family, the size in pixels, where on the tile the words sit, and whether they
//! are upper-cased. It lives here, in the profile, for the same reason
//! [`crate::Appearance`] does — a profile is the answer to "what does this deck
//! show", and a key's words are part of that answer. Two profiles can give the
//! same key two different fonts, and switching profile switches both.
//!
//! ## Why the defaults are not "no style"
//!
//! A key with no style of its own uses the app's default, which is kept in a
//! file of its own by the window (`title-style.json`). This type is what the
//! profile stores when a key *does* have one, so every field is concrete: there
//! is no `Option` to interpret here, and a file that omits a field gets the
//! default below rather than a hole.
//!
//! ## The size is a ceiling
//!
//! [`TitleStyle::size_px`] is the largest the words may be drawn, in pixels on
//! the deck's own tile. The drawing steps down when a title does not fit; it
//! never steps above this number. A size outside
//! [`MIN_SIZE_PX`]`..=`[`MAX_SIZE_PX`] is clamped on the way in, so a hand-edited
//! file cannot ask for a title no tile could hold.
//!
//! ## Example
//!
//! ```
//! use ss550_engine::{TitlePosition, TitleStyle};
//!
//! let style = TitleStyle {
//!     family: "Segoe UI".to_string(),
//!     size_px: 18,
//!     position: TitlePosition::Top,
//!     uppercase: true,
//! };
//! assert_eq!(style.size_px, 18);
//!
//! // A size no tile can hold is brought back into range on the way in: the
//! // profile clamps every style it stores.
//! let huge = TitleStyle::default().with_size(400);
//! assert_eq!(huge.size_px, ss550_engine::MAX_SIZE_PX);
//! ```

use serde::{Deserialize, Serialize};

/// The smallest a title's pixel size may be.
///
/// Below this a font's stems are thinner than a pixel and the words read as a
/// smudge rather than as a name.
pub const MIN_SIZE_PX: u16 = 6;

/// The largest a title's pixel size may be.
///
/// A key tile is about a hundred pixels on a side; a title larger than half of
/// it has no box to sit in, so the number is refused at the edge rather than
/// silently shrunk at the fit.
pub const MAX_SIZE_PX: u16 = 48;

/// The size a title is drawn at until someone picks another.
///
/// Fourteen pixels is two short lines in a caption band of the deck's own tile,
/// which is what most titles are.
pub const DEFAULT_SIZE_PX: u16 = 14;

/// Where on the tile a title sits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TitlePosition {
    /// A band along the top edge, over a picture.
    Top,
    /// A block in the middle, on the words themselves.
    Middle,
    /// A band along the bottom edge — the caption a picture usually carries.
    #[default]
    Bottom,
}

impl TitlePosition {
    /// Every position, in the order a settings page shows them.
    pub const ALL: [TitlePosition; 3] = [
        TitlePosition::Top,
        TitlePosition::Middle,
        TitlePosition::Bottom,
    ];
}

/// How one key's title is drawn.
///
/// Ordered so a cache can hold it in a key: two keys whose words are the same
/// but whose style differs are two different pictures, and this is what tells
/// them apart.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TitleStyle {
    /// The family the words are drawn in, by the name the system knows it by.
    ///
    /// Empty means "whatever sans the system defaults to": a profile written on
    /// Windows then still reads on a Mac, instead of naming a font that machine
    /// has never had.
    pub family: String,
    /// The size the words are drawn at, in pixels, and never more.
    pub size_px: u16,
    /// Where on the tile the words sit.
    pub position: TitlePosition,
    /// Whether the words are drawn in capitals.
    pub uppercase: bool,
}

impl Default for TitleStyle {
    fn default() -> Self {
        Self {
            family: String::new(),
            size_px: DEFAULT_SIZE_PX,
            position: TitlePosition::default(),
            uppercase: false,
        }
    }
}

impl TitleStyle {
    /// The same style at `px`, clamped to the range a tile can hold.
    ///
    /// Every path that sets a size goes through here — a typed number, a clicked
    /// one, and a file read from disk — so none of them can disagree about the
    /// range. Takes the style by shared reference so a caller can rewrite one
    /// field of a style it only borrows.
    pub fn with_size(&self, px: u16) -> Self {
        Self {
            size_px: px.clamp(MIN_SIZE_PX, MAX_SIZE_PX),
            ..self.clone()
        }
    }

    /// The same style with another family. An empty name is the system's own.
    pub fn with_family(&self, family: &str) -> Self {
        Self {
            family: family.to_string(),
            ..self.clone()
        }
    }

    /// This style with every field brought into the range a tile can hold.
    pub fn clamped(&self) -> Self {
        self.with_size(self.size_px)
    }
}

#[cfg(test)]
mod tests {
    use super::{TitlePosition, TitleStyle, DEFAULT_SIZE_PX, MAX_SIZE_PX, MIN_SIZE_PX};

    #[test]
    fn the_default_is_a_readable_caption_with_no_font_of_its_own() {
        let style = TitleStyle::default();
        assert_eq!(style.size_px, DEFAULT_SIZE_PX);
        assert!(style.family.is_empty(), "the system's sans, not a name");
        assert_eq!(style.position, TitlePosition::Bottom);
        assert!(!style.uppercase);
    }

    #[test]
    fn a_style_round_trips_through_json() {
        let style = TitleStyle {
            family: "Georgia".to_string(),
            size_px: 22,
            position: TitlePosition::Top,
            uppercase: true,
        };
        let json = serde_json::to_string(&style).expect("serialise");
        assert_eq!(
            json,
            r#"{"family":"Georgia","size_px":22,"position":"top","uppercase":true}"#
        );
        let read: TitleStyle = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(read, style);
    }

    #[test]
    fn a_style_that_omits_a_field_gets_the_default_for_it() {
        // The map in a profile is written by this build, but a hand-edited one
        // may name only the font: the rest has to fall back rather than fail.
        let read: TitleStyle = serde_json::from_str(r#"{"family":"Georgia"}"#).expect("a style");
        assert_eq!(read.family, "Georgia");
        assert_eq!(read.size_px, DEFAULT_SIZE_PX);
        assert_eq!(read.position, TitlePosition::Bottom);
        assert!(!read.uppercase);
    }

    #[test]
    fn an_empty_object_is_the_default_style() {
        let read: TitleStyle = serde_json::from_str("{}").expect("a style");
        assert_eq!(read, TitleStyle::default());
    }

    #[test]
    fn a_size_outside_the_range_is_clamped_whichever_way_it_goes() {
        let style = TitleStyle::default();
        assert_eq!(style.with_size(400).size_px, MAX_SIZE_PX);
        assert_eq!(style.with_size(0).size_px, MIN_SIZE_PX);
        assert_eq!(style.with_size(20).size_px, 20);
        // A value read from a hand-edited file goes through the same clamp.
        let read: TitleStyle = serde_json::from_str(r#"{"size_px":400}"#).expect("a style");
        assert_eq!(read.clamped().size_px, MAX_SIZE_PX);
    }

    #[test]
    fn two_styles_order_by_what_they_draw() {
        // The cache keys a composed picture by (words, style): two styles that
        // draw differently must not compare equal.
        let small = TitleStyle::default();
        let large = TitleStyle::default().with_size(30);
        assert_ne!(small, large);
        assert!(small < large || large < small, "a total order exists");
        let named = TitleStyle::default().with_family("Georgia");
        assert_ne!(named, TitleStyle::default());
    }
}

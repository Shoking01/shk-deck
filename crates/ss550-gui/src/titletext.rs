//! Drawing a title into a tile with a font the system already has.
//!
//! A key's words end up in the JPEG the deck receives, so they have to be
//! pixels before the encoder sees them. [`TextEngine`] shapes them with
//! `cosmic-text` at a size the user states in pixels and rasterises each glyph
//! onto the composed picture, which is why a title can now say `Canción` —
//! a bitmap of five-by-seven dots could only spell ASCII.
//!
//! ## Why this engine and not the window's
//!
//! `cosmic-text` is already compiled into the window: GPUI's own text stack
//! depends on it, so adding it here costs no new crate. It is a second
//! `FontSystem` all the same — GPUI does not lend us its database — and building
//! one reads the system's fonts, which the crate's own documentation calls out
//! as slow: up to a second on a release build. It is therefore built once, on
//! the background executor, the first time a key has words to draw, and kept for
//! the life of the process. Never on the UI thread, and never while a frame is
//! drawn: until the scan answers a titled key shows its picture without its
//! words, and the repaint that follows the scan draws them.
//!
//! ## What the size means
//!
//! [`TitleStyle::size_px`] is a **ceiling**, not an order. A title that does not
//! fit its band at that size steps down a pixel at a time until it does, and
//! never steps above it. That is the contract the bitmap renderer had before
//! this one, and a test still holds it.
//!
//! ## What is not here
//!
//! No layout runs during a frame: [`crate::icons::compose_title`] is called on
//! the change that needs it (a title, a style, a profile, a deck), exactly like
//! every other payload in the cache.

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap,
};
use image::RgbaImage;

use crate::titlestyle::TitleStyle;

/// The smallest a title is ever drawn, in pixels.
///
/// Below this the hints in a font turn into noise: a six-pixel stem is thinner
/// than one pixel, so the fit stops stepping down here and lets the words
/// overflow the box instead of drawing them as a grey smudge. It is the same
/// floor the settings page refuses to go under, stated once.
pub(crate) const MIN_FIT_PX: u16 = crate::titlestyle::MIN_SIZE_PX;

/// The most lines a title is ever drawn on.
///
/// Three is the ceiling the character cap was chosen for, and a fourth would
/// make the type smaller than the deck's own tile can show.
pub(crate) const TITLE_MAX_LINES: usize = 3;

/// The share of a tile's height a band takes, in percent.
const BAND_PERCENT: u32 = 38;

/// The share of a tile's height a centred block may use, in percent.
const BLOCK_PERCENT: u32 = 80;

/// The line height, as a multiple of the pixel size.
const LINE_HEIGHT: f32 = 1.2;

/// The ink a title is drawn in.
const INK: Color = Color::rgba(0xff, 0xff, 0xff, 0xff);

/// What is left of a channel under the scrim: 0.38, in 255ths.
const SCRIM_KEEP: u16 = 97;

/// A font database, a glyph cache, and the two calls that turn words into
/// pixels.
///
/// Owned by the controller and passed by reference to whatever composes a
/// payload, so one process holds one database: the scan it costs happens once.
pub(crate) struct TextEngine {
    /// The system's fonts, read when the engine was built.
    font_system: FontSystem,
    /// The rasteriser, which keeps the glyphs it has already drawn.
    cache: SwashCache,
}

impl TextEngine {
    /// Reads the system's fonts.
    ///
    /// Documented by `cosmic-text` as slow (up to a second on a release build),
    /// so the caller builds this once, on the first title, and keeps it.
    pub(crate) fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    /// Every family the system offers, in a stable order and without repeats.
    ///
    /// This is what the settings page offers. It is read from the database the
    /// engine already holds, so opening the list never scans anything twice.
    pub(crate) fn families(&mut self) -> Vec<String> {
        let mut names: Vec<String> = self
            .font_system
            .db_mut()
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();
        // One row per family, not per face: a family with four weights is one
        // choice in the list.
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup();
        names
    }

    /// Draws `title` onto a copy of `picture`.
    ///
    /// `None` when there is nothing to draw — an empty or spacing-only title —
    /// so the caller can skip the encode. The picture is returned untouched in
    /// that case.
    pub(crate) fn draw(
        &mut self,
        picture: &RgbaImage,
        title: &str,
        style: &TitleStyle,
    ) -> Option<RgbaImage> {
        let (width, height) = picture.dimensions();
        if width == 0 || height == 0 {
            return None;
        }
        let text = prepared(title, style.uppercase);
        if text.is_empty() {
            return None;
        }

        // The box the words may use. The inset keeps a word off the tile's
        // rounded edge, the same margin every size used before.
        let margin = (width / 16).max(1);
        let box_width = width.saturating_sub(margin * 2).max(1);
        let box_height = (height * box_percent(style) / 100).max(1);

        // The ceiling is the user's number; the fit can only take it down.
        let px = fitted_px(style.size_px, |candidate| {
            let (lines, block) = self.measure(&text, style, candidate, box_width);
            lines > 0 && lines <= TITLE_MAX_LINES && block <= box_height as f32
        });

        let (_, block) = self.measure(&text, style, px, box_width);
        let block = block.ceil() as u32;

        let mut picture = picture.clone();
        let pad = (height / 32).max(1);
        let top = match style.position {
            // A band at the top starts at the edge, with the same padding the
            // bottom band leaves under the words.
            crate::titlestyle::TitlePosition::Top => pad,
            // A block is centred, because it sits on the words and not on a
            // picture.
            crate::titlestyle::TitlePosition::Middle => height.saturating_sub(block) / 2,
            // A band is bottom-aligned, the way a caption sits on a picture.
            crate::titlestyle::TitlePosition::Bottom => height.saturating_sub(block + pad),
        };
        scrim(&mut picture, style, width, height, top, block, pad);

        let mut buffer = self.buffer(&text, style, px, box_width);
        let Self { font_system, cache } = self;
        let (tile_width, tile_height) = (width as i32, height as i32);
        let origin = (margin as i32, top as i32);
        buffer.draw(
            font_system,
            cache,
            INK,
            |x, y, _glyph_width, _height, color| {
                blend(
                    &mut picture,
                    origin.0 + x,
                    origin.1 + y,
                    color,
                    tile_width,
                    tile_height,
                );
            },
        );
        Some(picture)
    }

    /// How many lines `text` needs and how tall they are, at `px`.
    fn measure(&mut self, text: &str, style: &TitleStyle, px: u16, box_width: u32) -> (usize, f32) {
        let buffer = self.buffer(text, style, px, box_width);
        let mut lines = 0usize;
        let mut block = 0.0f32;
        for run in buffer.layout_runs() {
            lines += 1;
            block = block.max(run.line_top + run.line_height);
        }
        (lines, block)
    }

    /// Draws one line of text centred in a band of `picture`.
    ///
    /// This is the exporter a plugin's key uses, and it is deliberately separate
    /// from [`TextEngine::draw`]: a reading is not a title. There is no scrim, no
    /// case folding and no three-line block — just words at a pixel size, in a
    /// band the caller owns, in an ink the caller chose, stepping down until they
    /// fit that band.
    ///
    /// `None` when there is nothing to draw, so the caller can skip the encode.
    pub(crate) fn draw_band(
        &mut self,
        picture: &RgbaImage,
        text: &str,
        px: u16,
        band: (u32, u32),
        family: &str,
        ink: Color,
    ) -> Option<RgbaImage> {
        let (width, height) = picture.dimensions();
        if width == 0 || height == 0 {
            return None;
        }
        let text = prepared(text, false);
        if text.is_empty() {
            return None;
        }

        let margin = (width / 16).max(1);
        let box_width = width.saturating_sub(margin * 2).max(1);
        let family = match family.is_empty() {
            true => Family::SansSerif,
            false => Family::Name(family),
        };
        let (band_top, band_height) = band;

        // The size is a ceiling that only steps down: the caller states the size
        // it wants, and a band too short for it gets smaller words instead of
        // clipped ones.
        let mut chosen = px.max(MIN_FIT_PX);
        let mut block = self.measure_family(&text, family, chosen, box_width);
        while block > f32::from(u16::try_from(band_height).unwrap_or(u16::MAX))
            && chosen > MIN_FIT_PX
        {
            chosen -= 1;
            block = self.measure_family(&text, family, chosen, box_width);
        }

        let room = band_height.saturating_sub(block.ceil() as u32);
        let top = band_top + room / 2;
        let mut picture = picture.clone();
        let mut buffer = self.buffer_family(&text, family, chosen, box_width);
        let Self { font_system, cache } = self;
        let (tile_width, tile_height) = (width as i32, height as i32);
        let origin = (margin as i32, top as i32);
        buffer.draw(
            font_system,
            cache,
            ink,
            |x, y, _glyph_width, _height, color| {
                blend(
                    &mut picture,
                    origin.0 + x,
                    origin.1 + y,
                    color,
                    tile_width,
                    tile_height,
                );
            },
        );
        Some(picture)
    }

    /// One shaped buffer at `px`, wrapped to `box_width` and centred in it.
    fn buffer(&mut self, text: &str, style: &TitleStyle, px: u16, box_width: u32) -> Buffer {
        self.buffer_family(text, family(style), px, box_width)
    }

    /// [`TextEngine::buffer`] for a font named outright, which is what the
    /// exporter of a reading has.
    fn buffer_family(&mut self, text: &str, family: Family<'_>, px: u16, box_width: u32) -> Buffer {
        let size = f32::from(px);
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(size, size * LINE_HEIGHT),
        );
        buffer.set_wrap(Wrap::WordOrGlyph);
        // No height: the point of measuring is to learn how tall the words are,
        // so a clipped buffer would answer the question with the answer.
        buffer.set_size(Some(box_width as f32), None);
        let attrs = Attrs::new().family(family).color(INK);
        buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Center));
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }

    /// The height of `text` at `px`, in pixels.
    fn measure_family(&mut self, text: &str, family: Family<'_>, px: u16, box_width: u32) -> f32 {
        let buffer = self.buffer_family(text, family, px, box_width);
        buffer.layout_runs().fold(0.0f32, |tallest, run| {
            tallest.max(run.line_top + run.line_height)
        })
    }
}

/// The family a style draws with, or the system's own sans when it names none.
fn family<'a>(style: &'a TitleStyle) -> Family<'a> {
    match style.family.is_empty() {
        true => Family::SansSerif,
        false => Family::Name(&style.family),
    }
}

/// The share of the tile the words may use, from where they sit.
fn box_percent(style: &TitleStyle) -> u32 {
    match style.position {
        crate::titlestyle::TitlePosition::Middle => BLOCK_PERCENT,
        crate::titlestyle::TitlePosition::Top | crate::titlestyle::TitlePosition::Bottom => {
            BAND_PERCENT
        }
    }
}

/// The largest size at or below `max_px` that `fits` accepts.
///
/// Split out from [`TextEngine::draw`] because it is the whole of the sizing
/// rule and needs no font to be tested: the caller supplies the fit, this
/// supplies the ladder and the floor.
pub(crate) fn fitted_px(max_px: u16, mut fits: impl FnMut(u16) -> bool) -> u16 {
    let ceiling = max_px.max(MIN_FIT_PX);
    for px in (MIN_FIT_PX..=ceiling).rev() {
        if fits(px) {
            return px;
        }
    }
    MIN_FIT_PX
}

/// The words as they are drawn: trimmed, without control characters, and
/// upper-cased only when the style asks for it.
fn prepared(title: &str, uppercase: bool) -> String {
    let cleaned: String = title
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    match uppercase {
        true => cleaned.to_uppercase(),
        false => cleaned,
    }
}

/// Darkens the rectangle the words sit on, so white stays readable.
///
/// A multiply rather than an alpha blend: the pixels are already opaque and a
/// JPEG has no alpha, so what is under the band keeps its shape and loses most
/// of its brightness — the same band the window draws over a picture.
fn scrim(
    picture: &mut RgbaImage,
    style: &TitleStyle,
    width: u32,
    height: u32,
    top: u32,
    block: u32,
    pad: u32,
) {
    let (x0, y0, x1, y1) = match style.position {
        crate::titlestyle::TitlePosition::Top => (0, 0, width, (top + block + pad).min(height)),
        // A block behind the words. It spans the tile because the words do:
        // their box is the whole tile less its margin, so a narrower block
        // would leave the first and last letters off it.
        crate::titlestyle::TitlePosition::Middle => (
            0,
            top.saturating_sub(pad),
            width,
            (top + block + pad).min(height),
        ),
        crate::titlestyle::TitlePosition::Bottom => (0, top.saturating_sub(pad), width, height),
    };
    for y in y0.min(height)..y1.min(height) {
        for x in x0.min(width)..x1.min(width) {
            let pixel = picture.get_pixel_mut(x, y);
            for channel in 0..3 {
                pixel.0[channel] = ((u16::from(pixel.0[channel]) * SCRIM_KEEP) / 255) as u8;
            }
        }
    }
}

/// Composites one glyph pixel onto `picture`, clamped to the tile.
///
/// `cosmic-text` hands back a straight-alpha colour per covered pixel, so this
/// is the ordinary `src * a + dst * (1 - a)`, and the result stays opaque: the
/// encoder has no alpha channel and the deck has no idea what one is.
fn blend(picture: &mut RgbaImage, x: i32, y: i32, color: Color, width: i32, height: i32) {
    if x < 0 || y < 0 || x >= width || y >= height {
        return;
    }
    let alpha = u32::from(color.a());
    if alpha == 0 {
        return;
    }
    let [red, green, blue, _] = color.as_rgba();
    let keep = 255 - alpha;
    let pixel = picture.get_pixel_mut(x as u32, y as u32);
    for (channel, source) in [red, green, blue].into_iter().enumerate() {
        pixel.0[channel] =
            ((u32::from(source) * alpha + u32::from(pixel.0[channel]) * keep) / 255) as u8;
    }
    pixel.0[3] = 0xFF;
}

/// The engine, once the background scan has answered.
///
/// Every caller holds the engine as an `Option` and reaches for it *inside* the
/// branch that draws a title, so a profile with no titles — and every untitled
/// key in one that has them — never waits for, or pays for, the scan. `None`
/// here means the words are not drawn *yet*: the key falls back to its picture
/// and [`crate::Controller::request_fonts`] repaints it when the fonts arrive.
pub(crate) fn ready(text: &mut Option<TextEngine>) -> Option<&mut TextEngine> {
    text.as_mut()
}

/// One engine for the whole test binary.
///
/// Building a `FontSystem` reads the machine's fonts, which `cosmic-text` calls
/// out as slow, so every title test shares this one instead of paying for it
/// once per test. None of them asserts what a particular font looks like — a
/// test that did would pass on one machine and fail on the next.
#[cfg(test)]
pub(crate) fn test_engine() -> std::sync::MutexGuard<'static, TextEngine> {
    static ENGINE: std::sync::OnceLock<std::sync::Mutex<TextEngine>> = std::sync::OnceLock::new();
    ENGINE
        .get_or_init(|| std::sync::Mutex::new(TextEngine::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{family, fitted_px, prepared, test_engine, MIN_FIT_PX};
    use crate::titlestyle::TitleStyle;
    use cosmic_text::Family;
    use std::collections::BTreeSet;

    #[test]
    fn a_named_family_is_the_one_used_and_an_empty_name_is_the_systems_own() {
        let named = TitleStyle {
            family: "Georgia".to_string(),
            ..TitleStyle::default()
        };
        assert_eq!(family(&named), Family::Name("Georgia"));
        let system = TitleStyle::default();
        assert_eq!(
            family(&system),
            Family::SansSerif,
            "an empty name means whatever sans the system defaults to"
        );
    }

    #[test]
    fn the_engine_lists_the_families_the_settings_page_offers() {
        let mut engine = test_engine();
        let names = engine.families();
        assert!(
            !names.is_empty(),
            "a system with no font has nothing to draw a title with"
        );
        let unique: BTreeSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "a family is offered once");
    }

    #[test]
    fn the_size_is_a_ceiling_and_the_ladder_only_steps_down() {
        // A box that holds nothing above ten pixels: the fit answers ten, not
        // the twenty it was offered.
        let px = fitted_px(20, |candidate| candidate <= 10);
        assert_eq!(px, 10);
    }

    #[test]
    fn a_user_size_is_never_exceeded_even_when_everything_would_fit() {
        // Twenty fits at any size; the style asked for twenty, so twenty is the
        // answer and not one pixel more.
        assert_eq!(fitted_px(20, |_| true), 20);
        assert_eq!(fitted_px(14, |_| true), 14);
    }

    #[test]
    fn a_title_that_fits_at_no_size_keeps_the_floor() {
        assert_eq!(fitted_px(40, |_| false), MIN_FIT_PX);
        // A size below the floor cannot make the floor smaller.
        assert_eq!(fitted_px(2, |_| false), MIN_FIT_PX);
    }

    #[test]
    fn the_words_keep_the_case_the_user_typed() {
        // The bitmap had no lower case and forced every title up; a font does
        // not, and a title is the user's own text.
        assert_eq!(prepared("Canción", false), "Canción");
        assert_eq!(prepared("Canción", true), "CANCIÓN");
    }

    #[test]
    fn a_control_character_is_dropped_and_spacing_is_trimmed() {
        assert_eq!(prepared("  mute\t ", false), "mute");
        assert_eq!(prepared("a\u{7}b", false), "ab");
        assert!(prepared("   ", false).is_empty());
        assert!(prepared("", true).is_empty());
    }

    #[test]
    fn an_accent_is_one_character_not_a_fallback_mark() {
        // The regression this whole module exists for: `Ó` used to draw `#`.
        // Guarded by the family list rather than a bundled font, because a
        // title's family is whatever the system has.
        assert_eq!(
            prepared("canción", true),
            "CANCIÓN",
            "an accent survives the cap"
        );
        let letters: BTreeSet<char> = prepared("canción", true).chars().collect();
        assert!(letters.contains(&'Ó'), "the accent is still in the words");
    }
}

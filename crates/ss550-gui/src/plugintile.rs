//! The face of a key that shows a live reading.
//!
//! The deck receives one JPEG per key and nothing else: no text channel, no
//! font, no widget. So a plugin's reading has to be *drawn* — a label at the
//! top, the number in the middle, the line under it, and a bar when the number
//! has a scale — and encoded at the deck's own size, exactly like a title is.
//!
//! Everything here is a pure function of (cell, reading, size) so the same
//! reading always produces the same bytes. That is what makes the repaint rule
//! sound: a key may be skipped because its reading did not change, and the key
//! left on the deck is byte for byte what a repaint would have sent.

use image::{Rgba, RgbaImage};
use ss550_plugin_api::{CellSpec, Reading};

use crate::render;
use crate::titletext::{Band, TextEngine};

/// The face's background: near black, so the number is the brightest thing on
/// the key and the deck's backlight does not wash the value out.
const BACKGROUND: [u8; 3] = [0x0A, 0x0A, 0x0F];

/// The bar's empty track.
const TRACK: [u8; 3] = [0x22, 0x22, 0x2B];

/// The ink the number is drawn in.
const VALUE_INK: [u8; 3] = [0xF2, 0xF2, 0xF6];

/// The ink a number that could not be measured is drawn in: visible, and
/// unmistakably not a measurement.
const MISSING_INK: [u8; 3] = [0x6E, 0x6E, 0x7C];

/// The ink of the caption under the number.
const DETAIL_INK: [u8; 3] = [0x8E, 0x8E, 0x9C];

/// What each part of the face is, as a share of the tile.
///
/// Proportions rather than pixels, because a deck states its own image size and
/// the last two decks this window drove were 108 and 112 pixels square. The
/// numbers below are the only place the face's shape is decided; the comment on
/// each says what it is protecting, so tuning the face is reading four lines
/// instead of reverse-engineering a stack of divisions.
#[derive(Debug, Clone, Copy)]
struct Shares {
    /// Side margin, as a divisor of the width: the inset that keeps a word off
    /// the deck's rounded corner.
    margin_divisor: u32,
    /// The label band's height, as a divisor of the tile's height: one line of
    /// the smallest legible type.
    label_divisor: u32,
    /// The value band, as a percentage of the height: the number is the face, so
    /// it gets the largest share of it.
    value_percent: u32,
    /// The caption band, as a divisor of the height: one line, smaller than the
    /// label.
    detail_divisor: u32,
    /// The bar's height, as a divisor of the height.
    bar_divisor: u32,
    /// The gaps: between the label and the value, and under the value, as
    /// percentages of the height. They exist so the three bands never touch when
    /// a tile is at the small end of the range.
    gap_percent: u32,
    /// The type sizes, as divisors of the height: value, label, caption.
    value_type_divisor: u32,
    label_type_divisor: u32,
    detail_type_divisor: u32,
}

/// The face's proportions, in one place.
const SHARES: Shares = Shares {
    margin_divisor: 16,
    label_divisor: 6,
    value_percent: 38,
    detail_divisor: 8,
    bar_divisor: 16,
    gap_percent: 4,
    value_type_divisor: 3,
    label_type_divisor: 8,
    detail_type_divisor: 13,
};

/// The smallest type the face draws, in pixels.
///
/// Below this a stem is thinner than a pixel and the words turn into a smudge,
/// which is the same floor the title engine states for its own fit.
const MIN_TYPE_PX: u16 = 7;

/// Where each part of the face sits, in pixels, for one tile size.
struct Layout {
    label: Band,
    value: Band,
    detail: Band,
    /// The bar: its top and its height.
    bar: (u32, u32),
    /// The face's side margin, in pixels.
    margin: u32,
}

impl Layout {
    /// The layout a tile of this size gets.
    fn of(width: u32, height: u32) -> Self {
        let shares = SHARES;
        let margin = (width / shares.margin_divisor).max(1);
        let bar_height = (height / shares.bar_divisor).max(3);
        let label_height = (height / shares.label_divisor).max(10);
        let detail_height = (height / shares.detail_divisor).max(9);
        let value_height = (height * shares.value_percent / 100).max(12);
        let gap = (height * shares.gap_percent / 100).max(1);

        let value_top = label_height + gap;
        let detail_top = value_top + value_height + gap;
        let bar_top = height
            .saturating_sub(margin + bar_height)
            .max(detail_top + detail_height);

        // The type is a share of the *tile's* height, not of a band's: a band is
        // a fraction of the tile, and a fraction of a fraction is not a size.
        let type_px = |divisor: u32| {
            u16::try_from(height / divisor)
                .unwrap_or(u16::MAX)
                .max(MIN_TYPE_PX)
        };
        let band = |top: u32, height: u32, px: u16| Band {
            top,
            height,
            px,
            margin,
        };

        Self {
            label: band(margin / 2, label_height, type_px(shares.label_type_divisor)),
            value: band(value_top, value_height, type_px(shares.value_type_divisor)),
            detail: band(
                detail_top,
                detail_height,
                type_px(shares.detail_type_divisor),
            ),
            bar: (bar_top, bar_height),
            margin,
        }
    }
}

/// The pixels one key shows, before the encoder sees them.
///
/// Two faces, and the cell says which: a **gauge** (`Gauge`) is the label above,
/// the number, the caption under it and a bar; a **number** (`Number`) is the
/// reading alone, centred and as large as the tile allows, which is what a clock
/// key is. Which one is the plugin's statement, not the paint code's guess.
pub(crate) fn face(
    width: u32,
    height: u32,
    cell: &CellSpec,
    reading: &Reading,
    family: &str,
    text: &mut TextEngine,
) -> RgbaImage {
    let width = width.max(1);
    let height = height.max(1);
    let layout = Layout::of(width, height);
    let mut picture = RgbaImage::from_pixel(
        width,
        height,
        Rgba([BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 0xFF]),
    );

    let missing = reading.reason().is_some();
    let value_ink = match missing {
        true => MISSING_INK,
        false => VALUE_INK,
    };

    if cell.face() == ss550_plugin_api::Face::Number {
        return number(&picture, reading.text(), family, value_ink, text);
    }

    if cell.face() == ss550_plugin_api::Face::Title {
        return title_face(&picture, reading, family, value_ink, text);
    }

    // The bar goes under the words, so a caption that reaches too low covers
    // the track instead of the number growing a hole in it.
    if let Some(fraction) = reading.fraction() {
        draw_bar(&mut picture, &layout, cell.accent(), fraction);
    }

    let label_ink = match missing {
        true => MISSING_INK,
        false => cell.accent(),
    };

    let drawn = text.draw_band(&picture, cell.label(), layout.label, family, ink(label_ink));
    if let Some(next) = drawn {
        picture = next;
    }

    let drawn = text.draw_band(
        &picture,
        reading.text(),
        layout.value,
        family,
        ink(value_ink),
    );
    if let Some(next) = drawn {
        picture = next;
    }

    // A reading nothing measured says why where the caption goes: the reason is
    // the useful half of a dash, and it is drawn in the same place as the detail
    // line so the two cannot both claim it.
    let caption = reading.detail().or_else(|| reading.reason());
    if let Some(caption) = caption {
        let drawn = text.draw_band(&picture, caption, layout.detail, family, ink(DETAIL_INK));
        if let Some(next) = drawn {
            picture = next;
        }
    }

    picture
}

/// The words alone: the reading as a title, its detail under it.
///
/// A track name is not a number and does not fit like one. The title takes the
/// upper part of the tile and wraps — [`TextEngine::draw_band`] steps the size
/// down until it fits that band — and the artist sits in the strip below it,
/// smaller and dimmer, so the two are never the same weight. No label and no bar:
/// a player's key says what is playing, nothing else.
fn title_face(
    picture: &RgbaImage,
    reading: &Reading,
    family: &str,
    ink_rgb: [u8; 3],
    text: &mut TextEngine,
) -> RgbaImage {
    let (width, height) = picture.dimensions();
    let margin = (width / 12).max(1);
    let pad = (height / 16).max(1);
    let gap = (height / 40).max(1);

    // A player that publishes a cover gets the picture on the upper half and the
    // words under it; one that does not gets the title and the artist alone.
    let cover = reading
        .art()
        .and_then(decode_cover)
        .map(|cover| draw_cover(picture, &cover, width, pad, height * 52 / 100));

    let (picture, text_top) = match cover {
        Some(picture) => (picture, pad + (height * 52 / 100) + gap),
        None => (picture.clone(), pad),
    };

    let title_height = match reading.art().is_some() {
        // Under a cover: the strip that is left, split between the two lines.
        true => height.saturating_sub(text_top + pad).saturating_mul(62) / 100,
        false => (height * 58 / 100).max(1),
    };
    let artist_top = text_top + title_height + gap;
    let artist_height = height.saturating_sub(artist_top + pad).max(1);

    let title = Band {
        top: text_top,
        height: title_height.max(1),
        // The ceiling; the fit can only take it down.
        px: u16::try_from(height / 7)
            .unwrap_or(MIN_TYPE_PX)
            .max(MIN_TYPE_PX),
        margin,
    };
    let mut picture = text
        .draw_band(&picture, reading.text(), title, family, ink(ink_rgb))
        .unwrap_or(picture);

    // A reading nothing measured says why under the title, in the place the
    // artist would take: `nothing playing` is the useful half of a blank key.
    let caption = reading.detail().or_else(|| reading.reason());
    if let Some(caption) = caption {
        let artist = Band {
            top: artist_top,
            height: artist_height,
            px: u16::try_from(height / 14)
                .unwrap_or(MIN_TYPE_PX)
                .max(MIN_TYPE_PX),
            margin,
        };
        if let Some(next) = text.draw_band(&picture, caption, artist, family, ink(DETAIL_INK)) {
            picture = next;
        }
    }
    picture
}

/// The cover art, decoded, when the bytes are an image this build can read.
fn decode_cover(bytes: &[u8]) -> Option<RgbaImage> {
    image::load_from_memory(bytes)
        .ok()
        .map(|image| image.to_rgba8())
}

/// The cover, fitted whole inside the band and centred in it.
///
/// `contain`, not `cover`: a caratula cropped to a square slot loses the part
/// that names the album. The bands around it are the tile's own background, so a
/// narrow cover reads as a narrow picture on black rather than a stretched one.
fn draw_cover(
    picture: &RgbaImage,
    cover: &RgbaImage,
    width: u32,
    top: u32,
    band: u32,
) -> RgbaImage {
    let mut out = picture.clone();
    let (cover_width, cover_height) = cover.dimensions();
    if cover_width == 0 || cover_height == 0 || band == 0 {
        return out;
    }
    let scale = f32::min(
        width as f32 / cover_width as f32,
        band as f32 / cover_height as f32,
    );
    let fitted_width = ((cover_width as f32 * scale) as u32).clamp(1, width);
    let fitted_height = ((cover_height as f32 * scale) as u32).clamp(1, band);
    let fitted = image::imageops::resize(
        cover,
        fitted_width,
        fitted_height,
        image::imageops::FilterType::Triangle,
    );
    let left = width.saturating_sub(fitted_width) / 2;
    let top = top + band.saturating_sub(fitted_height) / 2;
    image::imageops::overlay(&mut out, &fitted, i64::from(left), i64::from(top));
    out
}

/// The number alone, centred on the tile, in the largest type that fits.
///
/// No label, no caption, no bar: a key that shows a time shows a time. The type
/// is fitted against **both** sides of the box, because a size that fits the
/// height but not the width wraps the number into two lines — and two `0`s
/// stacked is not a time.
fn number(
    picture: &RgbaImage,
    text_value: &str,
    family: &str,
    ink_rgb: [u8; 3],
    text: &mut TextEngine,
) -> RgbaImage {
    let (width, height) = picture.dimensions();
    let margin = (width / 8).max(1);
    let pad = (height / 16).max(1);
    let box_width = width.saturating_sub(margin * 2).max(1);
    let box_height = height.saturating_sub(pad * 2).max(1);

    // A digit is about 0.55 em wide, so this is roughly the size the box's width
    // can hold: the ladder only has to correct it by a few pixels either way
    // instead of walking down from the tile's height.
    let digits = u32::try_from(text_value.chars().count())
        .unwrap_or(1)
        .max(1);
    let estimate = box_width.saturating_mul(20) / (11 * digits);
    let ceiling = u16::try_from(estimate)
        .unwrap_or(u16::MAX)
        .min(u16::try_from(box_height).unwrap_or(u16::MAX))
        .max(MIN_TYPE_PX);
    let px = text.fit_one_line(text_value, family, ceiling, box_width, box_height);

    let band = Band {
        top: pad,
        height: box_height,
        px,
        margin,
    };
    text.draw_band(picture, text_value, band, family, ink(ink_rgb))
        .unwrap_or_else(|| picture.clone())
}

/// The deck's own payload for a face.
///
/// `None` when the deck's byte limit cannot hold it, or when the deck has not
/// stated a size yet — there is no payload to draw into.
pub(crate) fn encode(
    spec: deck_api::ImageSpec,
    cell: &CellSpec,
    picture: &RgbaImage,
) -> Option<Vec<u8>> {
    render::render_rgba(spec, cell.label(), picture).ok()
}

/// Paints the track and the filled part of the bar.
///
/// Every edge is clamped to the picture: a deck states a square size in the
/// hundreds, but `face` is written against `max(1)`, and a bar that reached one
/// column past a one-pixel picture would panic in the paint path instead of
/// drawing nothing.
fn draw_bar(picture: &mut RgbaImage, layout: &Layout, accent: [u8; 3], fraction: f32) {
    let (width, height) = picture.dimensions();
    let (top, bar_height) = layout.bar;
    let left = layout.margin.min(width);
    let right = width.saturating_sub(layout.margin).min(width).max(left);
    let span = right.saturating_sub(left);
    let filled = left + ((span as f32 * fraction.clamp(0.0, 1.0)).round() as u32);

    for y in top..top.saturating_add(bar_height).min(height) {
        for x in left..right {
            let colour = match x < filled {
                true => accent,
                false => TRACK,
            };
            picture.put_pixel(x, y, Rgba([colour[0], colour[1], colour[2], 0xFF]));
        }
    }
}

/// A colour, as the text engine wants it.
fn ink(rgb: [u8; 3]) -> cosmic_text::Color {
    cosmic_text::Color::rgba(rgb[0], rgb[1], rgb[2], 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::titletext::test_engine;
    use ss550_plugin_api::{CellId, PluginId};
    use std::sync::MutexGuard;

    fn cell() -> CellSpec {
        CellSpec::new(
            PluginId::parse("system-stats").expect("a usable id"),
            CellId::parse("cpu.usage").expect("a usable id"),
            "CPU",
            "Processor load, every core together",
            [0x38, 0xBD, 0xF8],
        )
    }

    fn engine() -> MutexGuard<'static, TextEngine> {
        test_engine()
    }

    #[test]
    fn a_cover_takes_the_top_half_and_the_words_go_under_it() {
        // Option B, as a test: the picture on top, the title and the artist
        // below it on the tile's own background.
        let mut text = engine();
        let clock = CellSpec::new(
            PluginId::parse("media").expect("a usable id"),
            CellId::parse("now").expect("a usable id"),
            "PLAY",
            "what the player is showing",
            [0x1d, 0xb9, 0x54],
        )
        .with_face(ss550_plugin_api::Face::Title);

        let cover = tiny_png(0xFF, 0x00, 0x00);
        let with_cover = face(
            112,
            112,
            &clock,
            &Reading::spelled("Windowlicker", 0.0)
                .with_detail("Aphex Twin")
                .with_art(cover),
            "",
            &mut text,
        );
        let words_only = face(
            112,
            112,
            &clock,
            &Reading::spelled("Windowlicker", 0.0).with_detail("Aphex Twin"),
            "",
            &mut text,
        );

        assert_ne!(
            with_cover.as_raw(),
            words_only.as_raw(),
            "a cover changed nothing about the face"
        );
        // The top half holds the picture: something that is not the background.
        let mut painted = 0;
        for y in 0..56 {
            for x in 0..112 {
                if with_cover.get_pixel(x, y).0[..3]
                    != [BACKGROUND[0], BACKGROUND[1], BACKGROUND[2]][..]
                {
                    painted += 1;
                }
            }
        }
        assert!(painted > 500, "the cover band is empty: {painted} pixels");
    }

    /// A one-colour PNG, built here so the test carries no image file.
    fn tiny_png(red: u8, green: u8, blue: u8) -> Vec<u8> {
        let image = RgbaImage::from_pixel(8, 8, Rgba([red, green, blue, 0xFF]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode the test cover");
        bytes.into_inner()
    }

    #[test]
    fn a_number_face_draws_the_number_and_nothing_else() {
        let mut text = engine();
        let accent = [0x60u8, 0x9a, 0xf0];
        let clock = CellSpec::new(
            PluginId::parse("clock").expect("a usable id"),
            CellId::parse("seconds").expect("a usable id"),
            "SEC",
            "the second, 00 to 59",
            accent,
        )
        .with_face(ss550_plugin_api::Face::Number);

        let picture = face(
            112,
            112,
            &clock,
            &Reading::spelled("07", 7.0),
            "",
            &mut text,
        );

        // The label and the bar are the two things the gauge draws in the
        // accent, and neither belongs on a key that is a time.
        assert!(
            picture.pixels().all(|pixel| pixel.0[..3] != accent[..]),
            "a label or a bar found its way onto a number"
        );
        // And the digits are there: something brighter than the background.
        assert!(
            picture
                .pixels()
                .any(|pixel| pixel.0[..3] != [BACKGROUND[0], BACKGROUND[1], BACKGROUND[2]][..]),
            "the number was not drawn"
        );
    }

    #[test]
    fn a_number_is_centred_and_larger_than_a_gauges_value() {
        let mut text = engine();
        let clock = CellSpec::new(
            PluginId::parse("clock").expect("a usable id"),
            CellId::parse("seconds").expect("a usable id"),
            "SEC",
            "the second, 00 to 59",
            [0x60, 0x9a, 0xf0],
        )
        .with_face(ss550_plugin_api::Face::Number);

        let number = face(
            112,
            112,
            &clock,
            &Reading::spelled("07", 7.0),
            "",
            &mut text,
        );
        let gauge = face(
            112,
            112,
            &clock.clone().with_face(ss550_plugin_api::Face::Gauge),
            &Reading::spelled("07", 7.0),
            "",
            &mut text,
        );

        let (left, top, right, bottom) = ink_box(&number);
        let centre = ((left + right) as i32 / 2, (top + bottom) as i32 / 2);
        // Centred in the tile, give or take the line box a font puts around its
        // digits: the ink sits a little above the middle of its own band.
        assert!(
            (centre.0 - 56).abs() <= 6,
            "the number is off-centre horizontally: {centre:?}"
        );
        assert!(
            (centre.1 - 56).abs() <= 16,
            "the number is off-centre vertically: {centre:?}"
        );

        let (_, gauge_top, _, gauge_bottom) = ink_box(&gauge);
        assert!(
            (bottom - top) > (gauge_bottom - gauge_top),
            "the number is not larger than a gauge's value: {} vs {}",
            bottom - top,
            gauge_bottom - gauge_top
        );
    }

    #[test]
    fn a_number_whose_family_is_missing_still_draws() {
        // The FONT control offers the system's families, but a profile can name
        // one this machine does not have: the key falls back to the sans the fit
        // started with instead of drawing a blank tile.
        let mut text = engine();
        let clock = CellSpec::new(
            PluginId::parse("clock").expect("a usable id"),
            CellId::parse("seconds").expect("a usable id"),
            "SEC",
            "the second, 00 to 59",
            [0x60, 0x9a, 0xf0],
        )
        .with_face(ss550_plugin_api::Face::Number);

        let picture = face(
            112,
            112,
            &clock,
            &Reading::spelled("07", 7.0),
            "A Family This Machine Does Not Have",
            &mut text,
        );

        let (left, top, right, bottom) = ink_box(&picture);
        assert!(right > left && bottom > top, "the number vanished");
    }

    /// The box that holds every pixel the face drew, as `(left, top, right, bottom)`.
    fn ink_box(picture: &RgbaImage) -> (u32, u32, u32, u32) {
        let background = [BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 0xFF];
        let (width, height) = picture.dimensions();
        let mut box_ = (width, height, 0u32, 0u32);
        for (x, y, pixel) in picture.enumerate_pixels() {
            if pixel.0 != background {
                box_.0 = box_.0.min(x);
                box_.1 = box_.1.min(y);
                box_.2 = box_.2.max(x);
                box_.3 = box_.3.max(y);
            }
        }
        box_
    }

    #[test]
    fn the_face_has_the_decks_own_size_and_its_background() {
        let mut text = engine();
        let picture = face(112, 112, &cell(), &Reading::percent(42.0), "", &mut text);
        assert_eq!(picture.dimensions(), (112, 112));
        assert_eq!(
            picture.get_pixel(1, 1).0,
            [BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 0xFF]
        );
    }

    #[test]
    fn the_bar_shows_the_percentage_and_nothing_when_there_is_no_percentage() {
        let mut text = engine();
        let layout = Layout::of(112, 112);

        let empty = face(112, 112, &cell(), &Reading::percent(0.0), "", &mut text);
        let full = face(112, 112, &cell(), &Reading::percent(100.0), "", &mut text);
        let (top, bar_height) = layout.bar;
        let row = top + bar_height / 2;
        assert_eq!(
            empty.get_pixel(60, row).0[..3],
            TRACK,
            "no percentage, no fill"
        );
        assert_eq!(
            full.get_pixel(60, row).0[..3],
            cell().accent(),
            "a full bar carries the cell's colour"
        );

        // A temperature has no scale, so its face has no bar at all.
        let heat = face(
            112,
            112,
            &cell(),
            &Reading::temperature(56.0),
            "",
            &mut text,
        );
        assert_eq!(heat.get_pixel(60, row).0[..3], BACKGROUND);
    }

    #[test]
    fn a_reading_nothing_measured_is_drawn_dimmer_than_a_number() {
        let mut text = engine();
        let measured = face(112, 112, &cell(), &Reading::percent(42.0), "", &mut text);
        let missing = face(
            112,
            112,
            &cell(),
            &Reading::unavailable("no sensor"),
            "",
            &mut text,
        );
        assert_ne!(
            measured.as_raw(),
            missing.as_raw(),
            "a dash and a reason are a different face from a number"
        );
    }

    #[test]
    fn the_same_reading_draws_the_same_bytes_so_a_skipped_repaint_is_safe() {
        let mut text = engine();
        let left = face(112, 112, &cell(), &Reading::percent(42.0), "", &mut text);
        let right = face(112, 112, &cell(), &Reading::percent(42.0), "", &mut text);
        assert_eq!(left.as_raw(), right.as_raw());

        let other = face(112, 112, &cell(), &Reading::percent(57.0), "", &mut text);
        assert_ne!(
            left.as_raw(),
            other.as_raw(),
            "another number is another face"
        );
    }

    #[test]
    fn the_faces_this_build_produces_fit_the_decks_byte_budget() {
        // The real faces are the ones with a caption: `12 threads`, `3D`,
        // `C: · 496.4 / 942.3 GB free`, or the reason a dash has. A budget proven
        // only on a bare number proves nothing about what the deck receives.
        let mut text = engine();
        let spec = deck_api::ImageSpec::new(112, 112, 4096, deck_api::ImageFormat::Jpeg);
        let faces = [
            Reading::percent(42.0).with_detail("12 threads"),
            Reading::percent(71.0).with_detail("11.3 / 15.9 GB"),
            Reading::percent(53.0).with_detail("C: · 496.4 / 942.3 GB free"),
            Reading::temperature(61.0).with_detail("LHM"),
            Reading::unavailable("no sensor"),
        ];
        for reading in faces {
            let picture = face(112, 112, &cell(), &reading, "", &mut text);
            let jpeg = encode(spec, &cell(), &picture).unwrap_or_else(|| {
                panic!(
                    "{:?} did not fit {} bytes",
                    reading.text(),
                    spec.max_bytes()
                )
            });
            assert!(jpeg.starts_with(&[0xFF, 0xD8]), "a JPEG starts with SOI");
            assert!(
                jpeg.len() <= spec.max_bytes(),
                "{:?} is {} bytes",
                reading.text(),
                jpeg.len()
            );
        }
    }

    #[test]
    fn the_face_encodes_at_the_decks_size() {
        let mut text = engine();
        let spec = deck_api::ImageSpec::new(112, 112, 4096, deck_api::ImageFormat::Jpeg);
        let picture = face(112, 112, &cell(), &Reading::percent(42.0), "", &mut text);
        let jpeg = encode(spec, &cell(), &picture).expect("a payload");
        assert!(jpeg.starts_with(&[0xFF, 0xD8]), "a JPEG starts with SOI");
        assert!(jpeg.len() <= spec.max_bytes());
    }

    #[test]
    fn the_bar_is_clamped_to_a_picture_smaller_than_its_own_margin() {
        // A deck states hundreds of pixels, but `face` accepts `max(1)` and the
        // bar must not reach past the picture: a panic here is a panicking
        // paint path, not a cosmetic bug.
        let mut text = engine();
        for (width, height) in [(1, 1), (1, 40), (4, 4), (8, 120)] {
            let picture = face(
                width,
                height,
                &cell(),
                &Reading::percent(50.0),
                "",
                &mut text,
            );
            assert_eq!(picture.dimensions(), (width, height));
        }
    }

    #[test]
    fn the_value_is_the_largest_type_and_the_caption_the_smallest() {
        let layout = Layout::of(112, 112);
        assert!(
            layout.value.px > layout.label.px,
            "the number is the face: {} is not larger than {}",
            layout.value.px,
            layout.label.px
        );
        assert!(
            layout.label.px > layout.detail.px,
            "the caption is the smallest line"
        );
    }

    #[test]
    fn every_part_of_the_face_stays_inside_the_tile() {
        // The layout is arithmetic on fractions, so the parts are checked
        // against each other: nothing overlaps the bar, and nothing leaves the
        // image, at the sizes a deck could state.
        for size in [96, 108, 112, 160] {
            let layout = Layout::of(size, size);
            assert!(
                layout.label.top + layout.label.height <= layout.value.top,
                "{size}"
            );
            assert!(
                layout.value.top + layout.value.height <= layout.detail.top,
                "{size}"
            );
            assert!(
                layout.detail.top + layout.detail.height <= layout.bar.0,
                "the caption must not sit on the bar at {size}"
            );
            assert!(layout.bar.0 + layout.bar.1 <= size, "{size}");
            assert!(layout.margin * 2 < size, "{size}");
            // The type is a ceiling that never falls below the legible floor.
            for band in [layout.label, layout.value, layout.detail] {
                assert!(band.px >= MIN_TYPE_PX, "{size}");
            }
        }
    }
}

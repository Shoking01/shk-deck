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

    // The bar goes under the words, so a caption that reaches too low covers
    // the track instead of the number growing a hole in it.
    if let Some(fraction) = reading.fraction() {
        draw_bar(&mut picture, &layout, cell.accent(), fraction);
    }

    let missing = reading.reason().is_some();
    let label_ink = match missing {
        true => MISSING_INK,
        false => cell.accent(),
    };
    let value_ink = match missing {
        true => MISSING_INK,
        false => VALUE_INK,
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

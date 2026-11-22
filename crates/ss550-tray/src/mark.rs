//! The program's mark: the deck's own face, as geometry.
//!
//! One face, three places. The window draws this mark from `div`s, the
//! executable carries it as an `.ico`, and the notification area carries it as
//! raw pixels — and all three read the numbers below, so a change here changes
//! every copy at once. Two hand-drawn copies of a logo are two logos within a
//! release, which is exactly the drift this module exists to prevent.
//!
//! The shape is the product: a rounded badge holding the deck's key grid, with
//! one key lit. It is not a letter and not a monogram — the deck is what the
//! program drives, and a grid of keys reads as one at sixteen pixels where a
//! wordmark does not.
//!
//! Everything is stated as a fraction of the badge's side, so the same numbers
//! draw a 16-pixel tray icon and a 1024-pixel store picture.

/// The keys across the badge.
pub const COLUMNS: usize = 5;

/// The keys down the badge.
pub const ROWS: usize = 3;

/// The key that is lit, counted left to right and top to bottom.
///
/// The middle key of the middle row: the one position that stays legible when
/// the whole grid is four pixels wide, and the one that reads as "the deck is
/// doing something" rather than as a decoration in a corner.
pub const LIT: usize = 7;

/// The badge's corner radius, as a fraction of its side.
pub const BADGE_RADIUS: f32 = 0.22;

/// One key's corner radius, as a fraction of the key's own side.
pub const KEY_RADIUS: f32 = 0.26;

/// One key: its left, its top, and its side, all fractions of the badge.
///
/// Five columns and three rows is the deck the program drives, and the gap is
/// the same both ways, so the band of keys is a grid and not a texture. The
/// band is centred: the margins above and below it are equal.
pub const KEYS: [(f32, f32, f32); COLUMNS * ROWS] = [
    (0.07, 0.25, 0.14),
    (0.25, 0.25, 0.14),
    (0.43, 0.25, 0.14),
    (0.61, 0.25, 0.14),
    (0.79, 0.25, 0.14),
    (0.07, 0.43, 0.14),
    (0.25, 0.43, 0.14),
    (0.43, 0.43, 0.14),
    (0.61, 0.43, 0.14),
    (0.79, 0.43, 0.14),
    (0.07, 0.61, 0.14),
    (0.25, 0.61, 0.14),
    (0.43, 0.61, 0.14),
    (0.61, 0.61, 0.14),
    (0.79, 0.61, 0.14),
];

/// The badge's colour at its top edge, as `0xRRGGBB`.
///
/// The artwork's own violet, a shade lighter than the accent so the badge has
/// the least possible depth: a gradient that is visible as a gradient is noise
/// at tray size.
pub const ARTWORK_TOP: u32 = 0x8d7ff3;

/// The badge's colour at its bottom edge.
pub const ARTWORK_BOTTOM: u32 = 0x6a59e0;

/// How solid a key that is not lit is drawn.
///
/// The lit key is the whole story of the mark, so the others step back: at full
/// white the grid reads as a filled square at sixteen pixels.
pub const KEY_INK: f32 = 0.55;

/// Whether `index` is the lit key.
///
/// A function rather than a comparison at each call site: the window and the
/// rasteriser both ask, and neither should spell the rule twice.
pub fn is_lit(index: usize) -> bool {
    index == LIT
}

/// The lit key's rectangle, for a caller that only wants the one.
pub fn lit_key() -> (f32, f32, f32) {
    KEYS[LIT]
}

#[cfg(test)]
mod tests {
    use super::{is_lit, lit_key, BADGE_RADIUS, COLUMNS, KEYS, KEY_RADIUS, LIT, ROWS};

    #[test]
    fn the_grid_holds_one_key_per_cell() {
        assert_eq!(KEYS.len(), COLUMNS * ROWS);
        assert!(LIT < KEYS.len(), "the lit key must be one of them");
        assert!(is_lit(LIT), "the lit index must be the lit key");
        assert_eq!(
            KEYS.iter()
                .enumerate()
                .filter(|(index, _)| is_lit(*index))
                .count(),
            1,
            "exactly one key is lit"
        );
    }

    #[test]
    fn every_key_sits_inside_the_badge() {
        for (left, top, side) in KEYS {
            assert!(left > 0. && top > 0. && side > 0.);
            assert!(
                left + side <= 1. && top + side <= 1.,
                "a key reaches outside the badge: {left} {top} {side}"
            );
        }
    }

    #[test]
    fn the_keys_are_one_uniform_grid() {
        // Every key is the same square, the gaps are equal both ways, and the
        // rows and columns line up: a grid with a stray number in it is the one
        // defect a reader cannot name but always sees.
        let (first_left, first_top, side) = KEYS[0];
        for (index, (left, top, key_side)) in KEYS.iter().copied().enumerate() {
            assert_eq!(key_side, side, "key {index} is a different size");
            let column = index % COLUMNS;
            let row = index / COLUMNS;
            assert!(
                (left - (first_left + column as f32 * (side + 0.04))).abs() < 1e-4,
                "key {index} is off its column"
            );
            assert!(
                (top - (first_top + row as f32 * (side + 0.04))).abs() < 1e-4,
                "key {index} is off its row"
            );
        }
    }

    #[test]
    fn the_band_of_keys_is_centred() {
        let top = KEYS[0].1;
        let bottom = KEYS[KEYS.len() - 1].1 + KEYS[0].2;
        let left = KEYS[0].0;
        let right = KEYS[COLUMNS - 1].0 + KEYS[0].2;

        assert!(
            (top - (1. - bottom)).abs() < 1e-4,
            "the band is not centred"
        );
        assert!(
            (left - (1. - right)).abs() < 1e-4,
            "the band is not centred"
        );
    }

    #[test]
    fn the_lit_key_is_the_middle_one() {
        let (left, top, side) = lit_key();

        assert!((left + side / 2. - 0.5).abs() < 1e-4);
        assert!((top + side / 2. - 0.5).abs() < 1e-4);
    }

    #[test]
    fn the_radii_are_rounding_and_not_circles() {
        // Both corners are a fraction of a side: too large and the badge is a
        // blob, too small and it is a rectangle with cut corners.
        for radius in [BADGE_RADIUS, KEY_RADIUS] {
            assert!(radius > 0.05, "a corner this tight is a cut, not a radius");
            assert!(radius < 0.5, "a corner this round is a circle");
        }
    }
}

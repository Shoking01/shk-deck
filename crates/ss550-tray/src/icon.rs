//! The tray icon, as pixels.
//!
//! The notification area draws its own copy: `tray-icon` takes straight RGBA and
//! the shell scales it to whatever the taskbar needs, so the crate carries the
//! pixels rather than an image file and a decoder.
//!
//! The picture is the application's own — a white `S` on a black-to-violet
//! gradient — reduced to the 32 pixels the tray is built at and masked to a
//! rounded badge. It is generated from
//! `crates/ss550-gui/assets/app-icon.png` by the same script that builds the
//! `.ico`, and regenerating is what keeps the two from drifting: a tray icon that
//! disagrees with the Start Menu entry is a program with two faces.

/// The width and height of the icon the tray is built from, in pixels.
pub const ICON_SIZE: u32 = 32;

/// One pixel per dot, RGBA, straight alpha, top row first.
///
/// Embedded rather than drawn here: the artwork is the application icon, and
/// keeping one copy of it is what stops the tray from showing a design the rest
/// of the program dropped.
const PIXELS: &[u8] = include_bytes!("../assets/tray-icon-32x32.rgba");

/// The tray icon's pixels, RGBA, ready for `tray_icon::Icon::from_rgba`.
///
/// # Example
///
/// ```
/// let pixels = ss550_tray::icon::deck_icon_rgba();
/// assert_eq!(pixels.len(), 32 * 32 * 4);
/// ```
pub fn deck_icon_rgba() -> Vec<u8> {
    PIXELS.to_vec()
}

/// The colour the badge is filled with when it carries a number.
///
/// Dark, because the digits have to be read at sixteen pixels on a taskbar that
/// may itself be light: the contrast has to come from the badge, not the theme.
const BADGE: [u8; 3] = [0x12, 0x12, 0x18];

/// How many characters a badge holds and still reads.
///
/// Three is the most a 32-pixel square can carry legibly once the shell halves
/// it for the taskbar: at four, the digits are one pixel wide and turn to mush.
pub const MAX_CHARS: usize = 3;

/// A digit, three dots wide and five tall, the top row first.
///
/// One `u8` per row, the leftmost dot in bit 2. Three by five is the smallest
/// shape that is still unmistakably a digit, and at the shell's scale it is the
/// difference between "59" and a smudge.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// How many pixels of the icon one dot of a digit covers.
///
/// The shell does not draw this at exactly half; on this taskbar a 32-pixel icon
/// comes out around twenty, and a filter with a ratio that is not a whole number
/// turns a one-pixel stroke into grey. Four pixels a dot — two on the taskbar —
/// is what survives that: a digit nine pixels wide and eleven tall, which is as
/// large as two of them fit in the badge.
const FAT: u32 = 4;

/// How many pixels a dot covers when three digits have to fit.
///
/// Three fat digits are wider than the badge, and a number that vanishes at the
/// hundreds is a number that vanishes exactly when it matters. It is drawn small
/// instead: less pretty, still there.
const THIN: u32 = 2;

/// The badge with `text` written on it, or `None` when it will not fit.
///
/// The ring is drawn here rather than taken from the application icon: the digits
/// need the whole square to be legible, so the silhouette is the rounded badge and
/// nothing else. Anything that is not one to [`MAX_CHARS`] digits is refused
/// instead of being drawn as a question mark: a number nobody can read is worse
/// than the plain icon.
///
/// # Example
///
/// ```
/// let icon = ss550_tray::icon::badge_with_digits("59", [255, 255, 255]).unwrap();
/// assert_eq!(icon.len(), 32 * 32 * 4);
/// assert!(ss550_tray::icon::badge_with_digits("12a", [255, 255, 255]).is_none());
/// ```
pub fn badge_with_digits(text: &str, colour: [u8; 3]) -> Option<Vec<u8>> {
    if text.is_empty() || text.chars().count() > MAX_CHARS {
        return None;
    }
    let glyphs = text
        .chars()
        .map(|character| {
            character
                .to_digit(10)
                .and_then(|digit| DIGITS.get(digit as usize).copied())
        })
        .collect::<Option<Vec<_>>>()?;

    let count = glyphs.len() as u32;
    let dot = if count <= 2 { FAT } else { THIN };
    let width = (count * 3 + (count - 1)) * dot;
    let height = 5 * dot;
    if width > ICON_SIZE || height > ICON_SIZE {
        return None;
    }

    // The badge, with the corners taken off so it sits beside the other icons as
    // a badge and not as a card.
    let mut pixels = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    const RADIUS: u32 = 6;
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            // How far inside the rounded corner the pixel is, on each axis.
            let inset = |at: u32| -> f32 {
                let from_near = RADIUS as f32 - at as f32;
                let from_far = RADIUS as f32 - (ICON_SIZE as f32 - 1.0 - at as f32);
                from_near.max(from_far).max(0.0)
            };
            let (dx, dy) = (inset(x), inset(y));
            if dx * dx + dy * dy > (RADIUS * RADIUS) as f32 {
                continue;
            }
            let offset = ((y * ICON_SIZE + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&[BADGE[0], BADGE[1], BADGE[2], 255]);
        }
    }

    let left = (ICON_SIZE - width) / 2;
    let top = (ICON_SIZE - height) / 2;
    for (index, glyph) in glyphs.iter().enumerate() {
        let cell = left + index as u32 * 4 * dot;
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..3u32 {
                if bits & (0b100 >> column) == 0 {
                    continue;
                }
                for dy in 0..dot {
                    for dx in 0..dot {
                        let x = cell + column * dot + dx;
                        let y = top + row as u32 * dot + dy;
                        let offset = ((y * ICON_SIZE + x) * 4) as usize;
                        pixels[offset..offset + 4]
                            .copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
                    }
                }
            }
        }
    }
    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pixel at a point of the icon, as RGBA.
    fn pixel_at(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
        let offset = (y * ICON_SIZE as usize + x) * 4;
        [
            pixels[offset],
            pixels[offset + 1],
            pixels[offset + 2],
            pixels[offset + 3],
        ]
    }

    #[test]
    fn the_icon_holds_one_rgba_pixel_per_dot() {
        assert_eq!(deck_icon_rgba().len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }

    #[test]
    fn the_corners_are_rounded_away() {
        // The notification area draws the icon straight onto the taskbar, so the
        // square is a badge and not a card: the corner pixel is transparent.
        let pixels = deck_icon_rgba();
        assert_eq!(pixel_at(&pixels, 0, 0)[3], 0, "the corner must be empty");
        assert_eq!(
            pixel_at(&pixels, 16, 2)[3],
            255,
            "the middle of the top edge must be filled"
        );
    }

    #[test]
    fn the_letter_is_white_and_the_background_is_violet() {
        // The mark is the S: a bright pixel has to exist, and the field behind
        // it has to be the gradient rather than the old red keys.
        let pixels = deck_icon_rgba();
        let bright = (0..ICON_SIZE as usize)
            .flat_map(|y| (0..ICON_SIZE as usize).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let pixel = pixel_at(&pixels, *x, *y);
                pixel[3] == 255 && pixel[0] > 200 && pixel[1] > 200
            })
            .count();
        assert!(bright > 20, "the letter must be there: {bright} pixels");

        let corner_of_the_field = pixel_at(&pixels, 16, 30);
        assert_eq!(corner_of_the_field[3], 255);
        assert!(
            corner_of_the_field[2] > corner_of_the_field[0],
            "the field is violet, not red: {corner_of_the_field:?}"
        );
    }

    /// How many pixels of the badge are exactly `colour` and opaque.
    fn count_of(pixels: &[u8], colour: [u8; 3]) -> usize {
        (0..ICON_SIZE as usize)
            .flat_map(|y| (0..ICON_SIZE as usize).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let pixel = pixel_at(pixels, *x, *y);
                pixel[3] == 255 && pixel[..3] == colour
            })
            .count()
    }

    #[test]
    fn a_number_is_drawn_on_a_dark_badge_with_the_same_silhouette() {
        let white = [0xff, 0xff, 0xff];
        let pixels = badge_with_digits("59", white).expect("two digits fit");
        assert_eq!(pixels.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        // 23 dots of two digits at sixteen pixels a dot.
        assert!(
            count_of(&pixels, white) >= 300,
            "the digits must be there: {}",
            count_of(&pixels, white)
        );
        // The field behind them is the dark badge, with no trace of the gradient.
        let field = pixel_at(&pixels, 16, 30);
        assert_eq!(field[..3], BADGE, "the field is the badge: {field:?}");
        // And the silhouette is the icon's own: the corner is still empty.
        assert_eq!(pixel_at(&pixels, 0, 0)[3], 0, "the corner must be empty");
    }

    #[test]
    fn a_badge_is_not_drawn_for_something_that_is_not_a_number() {
        let white = [0xff, 0xff, 0xff];
        assert!(badge_with_digits("", white).is_none(), "nothing to say");
        assert!(badge_with_digits("5a", white).is_none(), "not a digit");
        assert!(
            badge_with_digits("1234", white).is_none(),
            "four digits do not fit legibly, so they are refused rather than smeared"
        );
        // Three do fit, and that is the widest the badge goes.
        assert!(badge_with_digits("100", white).is_some());
    }

    #[test]
    fn one_digit_is_centred_and_not_left_in_a_corner() {
        let white = [0xff, 0xff, 0xff];
        let pixels = badge_with_digits("5", white).expect("one digit fits");
        let lit: Vec<(usize, usize)> = (0..ICON_SIZE as usize)
            .flat_map(|y| (0..ICON_SIZE as usize).map(move |x| (x, y)))
            .filter(|(x, y)| pixel_at(&pixels, *x, *y)[..3] == white)
            .collect();
        let left = lit.iter().map(|(x, _)| *x).min().expect("a digit");
        let right = lit.iter().map(|(x, _)| *x).max().expect("a digit");
        // `5` is twelve pixels wide at this scale, so it sits between 10 and 21.
        assert_eq!((left, right), (10, 21), "one digit is centred");
    }
}

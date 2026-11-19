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
}

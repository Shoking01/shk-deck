//! The tray icon, drawn in code.
//!
//! The icon is the deck itself: fifteen keys in a 3x5 grid on a dark rounded
//! square, with the middle key picked out. Drawing it here keeps the project
//! free of image files and of an image decoding dependency.

/// The width and height of the generated icon, in pixels.
pub const ICON_SIZE: u32 = 32;

/// The colour of the deck's keys.
const KEY_COLOR: [u8; 3] = [0xe5, 0x48, 0x4d];

/// The colour of the key in the middle, which stands out.
const HIGHLIGHT_COLOR: [u8; 3] = [0xff, 0xff, 0xff];

/// The colour behind the keys.
const BACKGROUND_COLOR: [u8; 3] = [0x16, 0x19, 0x1f];

/// How far the corners of the icon are rounded, in pixels.
const CORNER_RADIUS: i32 = 7;

/// Draws the tray icon and returns its pixels as RGBA, row by row.
///
/// # Example
///
/// ```no_run
/// let pixels = ss550_tray::icon::deck_icon_rgba();
/// assert_eq!(pixels.len(), 32 * 32 * 4);
/// ```
pub fn deck_icon_rgba() -> Vec<u8> {
    let size = ICON_SIZE as usize;
    let mut pixels = vec![0u8; size * size * 4];

    for y in 0..size {
        for x in 0..size {
            if inside_rounded_square(x, y) {
                set_pixel(&mut pixels, x, y, BACKGROUND_COLOR, 255);
            }
        }
    }

    // Fifteen keys, three columns by five rows, the way the deck is laid out.
    let key = 4usize;
    let gap = 2usize;
    let columns = 3usize;
    let rows = 5usize;
    let grid_width = columns * key + (columns - 1) * gap;
    let grid_height = rows * key + (rows - 1) * gap;
    let origin_x = (size - grid_width) / 2;
    let origin_y = (size - grid_height) / 2;

    for row in 0..rows {
        for column in 0..columns {
            let x = origin_x + column * (key + gap);
            let y = origin_y + row * (key + gap);
            let color = if row == 2 && column == 1 {
                HIGHLIGHT_COLOR
            } else {
                KEY_COLOR
            };
            for offset_y in 0..key {
                for offset_x in 0..key {
                    set_pixel(&mut pixels, x + offset_x, y + offset_y, color, 255);
                }
            }
        }
    }

    pixels
}

/// Whether the pixel is inside the rounded square that backs the keys.
fn inside_rounded_square(x: usize, y: usize) -> bool {
    let last = ICON_SIZE as i32 - 1;
    let x = x as i32;
    let y = y as i32;
    // The closest point of the inner rectangle: a pixel outside it is only
    // inside the square when it is within the radius of a corner.
    let closest_x = x.clamp(CORNER_RADIUS, last - CORNER_RADIUS);
    let closest_y = y.clamp(CORNER_RADIUS, last - CORNER_RADIUS);
    let distance_x = x - closest_x;
    let distance_y = y - closest_y;
    distance_x * distance_x + distance_y * distance_y <= CORNER_RADIUS * CORNER_RADIUS
}

/// Writes one RGBA pixel.
fn set_pixel(pixels: &mut [u8], x: usize, y: usize, color: [u8; 3], alpha: u8) {
    let offset = (y * ICON_SIZE as usize + x) * 4;
    pixels[offset] = color[0];
    pixels[offset + 1] = color[1];
    pixels[offset + 2] = color[2];
    pixels[offset + 3] = alpha;
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
        let pixels = deck_icon_rgba();
        assert_eq!(pixel_at(&pixels, 0, 0)[3], 0, "the corner must be empty");
        assert_eq!(
            pixel_at(&pixels, 3, 3)[3],
            255,
            "the square must be filled just inside the corner"
        );
    }

    #[test]
    fn the_middle_key_is_picked_out() {
        let pixels = deck_icon_rgba();
        assert_eq!(
            pixel_at(&pixels, 15, 15),
            [255, 255, 255, 255],
            "the key in the middle must be the bright one"
        );
    }

    #[test]
    fn the_keys_sit_on_the_dark_background() {
        let pixels = deck_icon_rgba();
        assert_eq!(
            pixel_at(&pixels, 8, 2),
            [0xe5, 0x48, 0x4d, 0xff],
            "the first key must be a red one"
        );
        assert_eq!(
            pixel_at(&pixels, 12, 3),
            [0x16, 0x19, 0x1f, 0xff],
            "the gap between the keys must show the background"
        );
    }
}

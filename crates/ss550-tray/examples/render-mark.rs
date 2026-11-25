//! Rasterises the program's mark into the three copies the build carries.
//!
//! `cargo run --example render-mark` in this crate rewrites:
//!
//! - `crates/ss550-gui/assets/app-icon.png`, the store-sized artwork;
//! - `crates/ss550-gui/assets/app-icon.ico`, what the executable, the taskbar,
//!   the Start Menu entry and Explorer all show;
//! - `crates/ss550-tray/assets/tray-icon-32x32.rgba`, the notification area's
//!   own pixels.
//!
//! All three come from [`ss550_tray::mark`], the same geometry the window draws
//! its own mark from, so the program cannot end up with two faces. Nothing here
//! runs at build time: the outputs are committed, and this is how they are
//! regenerated when the mark changes.
//!
//! The drawing is done here rather than in an image editor because the shapes
//! are a handful of rounded rectangles: a script that draws them is a script
//! that can draw them again, at any size, without a human lining up pixels.

use image::{Rgba, RgbaImage};
use ss550_tray::mark;

/// How many samples per axis one pixel is tested with.
///
/// Four is enough for a shape made of straight edges and quarter circles: the
/// worst case is a corner that lands mid-pixel, and the error is a sixteenth of
/// a pixel.
const SAMPLES: u32 = 4;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let gui_assets = root.join("../ss550-gui/assets");
    let tray_assets = root.join("assets");

    let large = render(1024);
    large
        .save_with_format(gui_assets.join("app-icon.png"), image::ImageFormat::Png)
        .expect("the 1024 artwork is writable");

    let ico = icon_file(&[16, 32, 48, 256]);
    std::fs::write(gui_assets.join("app-icon.ico"), ico).expect("the .ico is writable");

    let tray = render(32);
    let mut raw = Vec::with_capacity(32 * 32 * 4);
    for pixel in tray.pixels() {
        raw.extend_from_slice(&pixel.0);
    }
    std::fs::write(tray_assets.join("tray-icon-32x32.rgba"), raw)
        .expect("the tray pixels are writable");

    println!("wrote app-icon.png, app-icon.ico and tray-icon-32x32.rgba");
}

/// The mark, drawn at `side` pixels square.
fn render(side: u32) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
    let size = side as f32;

    // The badge: the artwork's own violet, a shade lighter at the top.
    let top = channels(mark::ARTWORK_TOP);
    let bottom = channels(mark::ARTWORK_BOTTOM);
    fill_rounded(
        &mut canvas,
        (0., 0., size, size),
        mark::BADGE_RADIUS * size,
        |_, y| {
            let t = y / size.max(1.);
            [
                top[0] + (bottom[0] - top[0]) * t,
                top[1] + (bottom[1] - top[1]) * t,
                top[2] + (bottom[2] - top[2]) * t,
                1.,
            ]
        },
    );

    // The keys, in the same places the window puts them.
    for (index, (left, top, key)) in mark::KEYS.iter().copied().enumerate() {
        let ink = if mark::is_lit(index) {
            1.
        } else {
            mark::KEY_INK
        };
        fill_rounded(
            &mut canvas,
            (left * size, top * size, key * size, key * size),
            mark::KEY_RADIUS * key * size,
            |_, _| [1., 1., 1., ink],
        );
    }

    canvas
}

/// One `0xRRGGBB` colour as three floats in `0..1`.
fn channels(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xff) as f32 / 255.,
        ((hex >> 8) & 0xff) as f32 / 255.,
        (hex & 0xff) as f32 / 255.,
    ]
}

/// Fills a rounded rectangle, blended over what is already there.
///
/// `color_at` is asked for the colour at a sample's position, which is what
/// makes the badge's gradient one fill instead of one fill per row.
fn fill_rounded(
    canvas: &mut RgbaImage,
    (x, y, width, height): (f32, f32, f32, f32),
    radius: f32,
    color_at: impl Fn(f32, f32) -> [f32; 4],
) {
    let left = x.floor().max(0.) as i64;
    let top = y.floor().max(0.) as i64;
    let right = (x + width).ceil().min(canvas.width() as f32) as i64;
    let bottom = (y + height).ceil().min(canvas.height() as f32) as i64;
    let step = 1. / SAMPLES as f32;

    for py in top..bottom {
        for px in left..right {
            let mut covered = 0.;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let sample_x = px as f32 + (sx as f32 + 0.5) * step;
                    let sample_y = py as f32 + (sy as f32 + 0.5) * step;
                    if inside_rounded(sample_x, sample_y, (x, y, width, height), radius) {
                        covered += 1.;
                    }
                }
            }
            let coverage = covered / (SAMPLES * SAMPLES) as f32;
            if coverage <= 0. {
                continue;
            }
            let color = color_at(px as f32 + 0.5, py as f32 + 0.5);
            blend(canvas, px as u32, py as u32, color, coverage);
        }
    }
}

/// Whether a point is inside a rounded rectangle.
///
/// The four corners are quarter circles of `radius`; the rest of the shape is
/// the plain rectangle between them.
fn inside_rounded(
    x: f32,
    y: f32,
    (left, top, width, height): (f32, f32, f32, f32),
    radius: f32,
) -> bool {
    if x < left || y < top || x >= left + width || y >= top + height {
        return false;
    }
    let radius = radius.min(width / 2.).min(height / 2.);
    // The nearest corner centre, in the direction of the point.
    let cx = x.clamp(left + radius, left + width - radius);
    let cy = y.clamp(top + radius, top + height - radius);
    let (dx, dy) = (x - cx, y - cy);

    dx * dx + dy * dy <= radius * radius
}

/// Composites one colour over a pixel, straight alpha, `coverage` of it.
fn blend(canvas: &mut RgbaImage, x: u32, y: u32, color: [f32; 4], coverage: f32) {
    let destination = canvas.get_pixel(x, y).0;
    let src_alpha = color[3] * coverage;
    let dst_alpha = f32::from(destination[3]) / 255.;
    let out_alpha = src_alpha + dst_alpha * (1. - src_alpha);
    if out_alpha <= 0. {
        canvas.put_pixel(x, y, Rgba([0, 0, 0, 0]));
        return;
    }

    let mut out = [0u8; 4];
    for channel in 0..3 {
        let src = color[channel];
        let dst = f32::from(destination[channel]) / 255.;
        let value = (src * src_alpha + dst * dst_alpha * (1. - src_alpha)) / out_alpha;
        out[channel] = (value.clamp(0., 1.) * 255.).round() as u8;
    }
    out[3] = (out_alpha.clamp(0., 1.) * 255.).round() as u8;
    canvas.put_pixel(x, y, Rgba(out));
}

/// A Windows icon holding one PNG per size.
///
/// PNG payloads rather than the older bitmap ones: every Windows that can run
/// this program reads them, and they cost a fraction of the uncompressed form
/// at 256 pixels.
fn icon_file(sizes: &[u32]) -> Vec<u8> {
    let mut payloads = Vec::new();
    for size in sizes {
        let mut png = std::io::Cursor::new(Vec::new());
        render(*size)
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("the icon size is encodable");
        payloads.push(png.into_inner());
    }

    let mut out = Vec::new();
    // ICONDIR: reserved, type 1 (icon), how many pictures follow.
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());

    // One directory entry per picture; the payloads start after all of them.
    let mut offset = 6 + 16 * sizes.len();
    for (size, payload) in sizes.iter().zip(&payloads) {
        // 256 is written as 0: the field is one byte and the convention is the
        // only way it can hold the largest size.
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(if *size >= 256 { 0 } else { *size as u8 });
        out.push(0); // palette size: true colour
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += payload.len();
    }

    for payload in payloads {
        out.extend_from_slice(&payload);
    }
    out
}

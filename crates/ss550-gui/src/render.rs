//! Turning a picture in the asset library into the bytes one key accepts.
//!
//! The deck states what it takes — a width, a height, a byte limit and an
//! encoding — and this module is the only place that answers it. Nothing here
//! hard-codes a tile size: the deck's `ImageSpec` is passed in, so a deck that
//! takes 100×100, 72×72, or any other square is served by the same code.
//!
//! ## What "fitted" means
//!
//! A picture that is not the deck's shape is **fitted**, never stretched: the
//! aspect ratio is kept, the result is centred, and what is left over is black.
//! Stretching a face into a square is the kind of thing a user notices and
//! cannot fix.
//!
//! ## What happens when it is too big
//!
//! A photographic tile can exceed the deck's byte limit at high quality. Rather
//! than send a payload the deck will refuse, the tile is re-encoded at a lower
//! quality — see [`QUALITY_LADDER`] — and only when even the floor does not fit
//! is the tile refused, naming the size obtained and the size allowed.
//!
//! ## Example
//!
//! ```text
//! let tile = render(spec, &name, &png_bytes)?;
//! assert!(tile.len() <= spec.max_bytes());
//! ```

use std::collections::BTreeMap;

use deck_api::ImageSpec;
use image::{imageops, Frame, Rgb, RgbImage, Rgba, RgbaImage};
use jpeg_encoder::{ColorType, Encoder};
use ss550_engine::AssetName;

/// The qualities tried, in order, before a tile is refused.
///
/// The deck's byte limit, not this list, is what decides how good a tile may
/// look: a 100×100 picture at 95 is usually well inside nine kilobytes, and
/// spending 80 there throws away detail the deck had room for. The ladder
/// therefore starts at the top and steps down only when a rung does not fit; the
/// floor is low enough that a noisy photograph still lands in a few kilobytes,
/// and below it the artefacts are worse than the refusal.
pub const QUALITY_LADDER: [u8; 7] = [95, 88, 80, 65, 50, 35, 20];

/// Why a picture could not become a tile.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The deck asked for an image with no pixels in it.
    #[error("the deck asks for a {width}x{height} image, which has no pixels")]
    EmptySpec {
        /// The width the deck declared.
        width: u16,
        /// The height the deck declared.
        height: u16,
    },

    /// The bytes are not an image this build can read.
    #[error("{name} is not a readable image: {source}")]
    Decode {
        /// The asset that failed to decode.
        name: String,
        /// The decoder's own reason.
        source: image::ImageError,
    },

    /// The tile does not fit in the deck's payload limit, even at the floor.
    #[error(
        "{name} is {bytes} bytes at the lowest quality, and this deck accepts at most {max_bytes}"
    )]
    TooLarge {
        /// The asset that did not fit.
        name: String,
        /// The smallest encoding this pipeline could produce.
        bytes: usize,
        /// The largest payload the deck accepts.
        max_bytes: usize,
    },

    /// The tile could not be encoded.
    #[error("could not encode the key image: {0}")]
    Encode(#[from] jpeg_encoder::EncodingError),
}

/// Decodes `source`, fits it to `spec` and encodes it as the deck's format.
///
/// # Errors
///
/// [`RenderError::Decode`] when the bytes are not an image, and
/// [`RenderError::TooLarge`] when even [`QUALITY_LADDER`]'s floor exceeds the
/// deck's byte limit. The refusal names the size obtained and the size allowed,
/// so the window can tell the user what to do about it.
pub fn render(spec: ImageSpec, name: &AssetName, source: &[u8]) -> Result<Vec<u8>, RenderError> {
    render_with_ladder(spec, name, source, &QUALITY_LADDER)
}

/// [`render`], with the qualities to try spelled out.
///
/// Split out so a test can force the refusal path and prove the ladder steps
/// down, without having to build a picture that happens to straddle a limit.
fn render_with_ladder(
    spec: ImageSpec,
    name: &AssetName,
    source: &[u8],
    ladder: &[u8],
) -> Result<Vec<u8>, RenderError> {
    // A deck that asks for no pixels is refused before an image is decoded:
    // there is nothing to fit anything into.
    let (width, height) = (spec.width(), spec.height());
    if width == 0 || height == 0 {
        return Err(RenderError::EmptySpec { width, height });
    }

    let decoded = image::load_from_memory(source).map_err(|source| RenderError::Decode {
        name: name.to_string(),
        source,
    })?;

    // Some decks mount their display upside down and say so in their protocol
    // notes, so the encoded tile is flipped once here, before it is fitted:
    // every caller then sends bytes the deck shows the right way up.
    let rgb = decoded.to_rgb8();
    encode_tile(spec, &name.to_string(), &rgb, ladder)
}

/// The deck's payload for pixels this build already holds.
///
/// A program's icon is decoded by Windows, not by an image codec, so there are
/// no bytes to read: [`render`] would have to be handed a re-encoded copy of a
/// picture that is already in memory. This is the same pipeline — the same
/// flip, the same fit, the same quality ladder — entered one step later.
///
/// # Errors
///
/// [`RenderError::EmptySpec`] when the deck asks for no pixels, and
/// [`RenderError::TooLarge`] when even [`QUALITY_LADDER`]'s floor exceeds the
/// deck's byte limit.
pub fn render_rgba(
    spec: ImageSpec,
    name: &str,
    picture: &RgbaImage,
) -> Result<Vec<u8>, RenderError> {
    let rgb = on_black(picture);
    encode_tile(spec, name, &rgb, &QUALITY_LADDER)
}

/// The tile one deck accepts, from a picture already in RGB.
fn encode_tile(
    spec: ImageSpec,
    name: &str,
    picture: &RgbImage,
    ladder: &[u8],
) -> Result<Vec<u8>, RenderError> {
    let (width, height) = (spec.width(), spec.height());
    if width == 0 || height == 0 {
        return Err(RenderError::EmptySpec { width, height });
    }

    // The deck's display is mounted the other way up; the flip is stated once,
    // here, for every picture that reaches it.
    let rotated = imageops::rotate180(picture);
    let canvas = fit(spec, &rotated);

    let mut smallest = 0;
    for quality in ladder.iter().copied() {
        let mut jpeg = Vec::new();
        Encoder::new(&mut jpeg, quality).encode(canvas.as_raw(), width, height, ColorType::Rgb)?;
        if jpeg.len() <= spec.max_bytes() {
            return Ok(jpeg);
        }
        smallest = jpeg.len();
    }

    Err(RenderError::TooLarge {
        name: name.to_string(),
        bytes: smallest,
        max_bytes: spec.max_bytes(),
    })
}

/// An RGBA picture flattened onto black, in straight alpha.
///
/// A key has no background of its own and a JPEG has no alpha channel, so a
/// transparent pixel becomes the same black the fit path leaves in its bands.
/// The multiply happens here, once: what reaches the deck and the tile is
/// opaque, and nothing downstream has to know what premultiplied means.
pub fn on_black(picture: &RgbaImage) -> RgbImage {
    let (width, height) = picture.dimensions();
    let mut flat = RgbImage::new(width, height);
    for (x, y, pixel) in picture.enumerate_pixels() {
        let alpha = u16::from(pixel[3]);
        let channel = |value: u8| ((u16::from(value) * alpha) / 255) as u8;
        flat.put_pixel(
            x,
            y,
            Rgb([channel(pixel[0]), channel(pixel[1]), channel(pixel[2])]),
        );
    }
    flat
}

/// Scales `picture` to fit inside the deck's image, centred on black.
///
/// The aspect ratio is kept: the smaller of the two ratios decides the scale, so
/// the picture always fits and the leftover bands are painted black.
fn fit(spec: ImageSpec, picture: &RgbImage) -> RgbImage {
    let (width, height) = (u32::from(spec.width()), u32::from(spec.height()));
    let (source_width, source_height) = picture.dimensions();

    let mut canvas = RgbImage::from_pixel(width, height, Rgb([0, 0, 0]));
    if source_width == 0 || source_height == 0 {
        return canvas;
    }

    let scale = (f64::from(width) / f64::from(source_width))
        .min(f64::from(height) / f64::from(source_height));
    let fitted_width = (f64::from(source_width) * scale)
        .round()
        .clamp(1.0, f64::from(width)) as u32;
    let fitted_height = (f64::from(source_height) * scale)
        .round()
        .clamp(1.0, f64::from(height)) as u32;

    let fitted = imageops::resize(
        picture,
        fitted_width,
        fitted_height,
        imageops::FilterType::Triangle,
    );
    let left = i64::from((width - fitted_width) / 2);
    let top = i64::from((height - fitted_height) / 2);
    imageops::overlay(&mut canvas, &fitted, left, top);

    canvas
}

/// The tile one key would receive, as a frame the window can draw.
///
/// This is the **fitted** tile, not the source picture: what the picker shows
/// is what the deck would show, black bands and all. A preview that cropped or
/// stretched would be a promise the deck does not keep.
///
/// The frame is in RGBA, which is what `RenderImage::new` expects. The GUI
/// library takes care of any format conversion needed for display.
///
/// # Errors
///
/// The same as [`render`], minus the size refusal: a preview is not a payload,
/// so a picture that would be too big to send is still worth looking at.
pub fn preview_frame(
    spec: ImageSpec,
    name: &AssetName,
    source: &[u8],
) -> Result<Frame, RenderError> {
    let (width, height) = (spec.width(), spec.height());
    if width == 0 || height == 0 {
        return Err(RenderError::EmptySpec { width, height });
    }

    let decoded = image::load_from_memory(source).map_err(|source| RenderError::Decode {
        name: name.to_string(),
        source,
    })?;

    // The picture is fitted as it is: this is the preview of what the deck
    // *shows*, and the deck's own display is mounted the other way up, which is
    // why the payload is flipped on the way out. Flipping the preview too would
    // show the tile upside down next to a key that is the right way up.
    let rgb = decoded.to_rgb8();
    let fitted = fit(spec, &rgb);
    Ok(frame_from_rgb(&fitted))
}

/// [`preview_frame`] for pixels this build already holds.
///
/// The same tile the deck would receive from [`render_rgba`], letterboxing and
/// all, so a key that draws its program's icon draws exactly what it sends.
pub fn preview_frame_rgba(spec: ImageSpec, picture: &RgbaImage) -> Result<Frame, RenderError> {
    let (width, height) = (spec.width(), spec.height());
    if width == 0 || height == 0 {
        return Err(RenderError::EmptySpec { width, height });
    }
    let fitted = fit(spec, &on_black(picture));
    Ok(frame_from_rgb(&fitted))
}

/// Pixels this build already holds, at their own size, as the window's frame.
///
/// Used when there is no deck to state an image size: the window still has to
/// show what a key would run, and a picture scaled to a deck that is not there
/// would be a promise nobody asked for.
pub fn frame_from_rgba(picture: &RgbaImage) -> Frame {
    frame_from_rgb(&on_black(picture))
}

/// One RGB picture as the renderer's own frame.
///
/// The renderer takes its pixels in BGRA, alpha first byte last, the same
/// layout it produces when it decodes a PNG itself. A `Frame` is built from an
/// RGBA buffer, so the red and blue channels are swapped once here; handing it
/// RGBA gives a preview whose reds and blues are exchanged. Opaque alpha is its
/// own premultiplied value, so the bands stay black.
fn frame_from_rgb(fitted: &RgbImage) -> Frame {
    let (w, h) = fitted.dimensions();
    let mut rgba = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let pixel = fitted.get_pixel(x, y);
            rgba.put_pixel(x, y, Rgba([pixel[2], pixel[1], pixel[0], 0xFF]));
        }
    }
    Frame::new(rgba)
}

/// The tiles already rendered, so a profile switch does not re-decode them.
///
/// An entry is keyed by the asset's name, which is enough because the library
/// never replaces a name: [`AssetLibrary::import`] refuses to overwrite, so the
/// bytes behind a name cannot change behind the cache's back. A file the user
/// edits by hand is picked up after [`TileCache::invalidate`] or
/// [`TileCache::clear`].
///
/// [`AssetLibrary::import`]: ss550_engine::AssetLibrary::import
pub struct TileCache {
    spec: ImageSpec,
    tiles: BTreeMap<AssetName, Vec<u8>>,
}

impl TileCache {
    /// A cache for the tiles one deck accepts.
    pub fn new(spec: ImageSpec) -> Self {
        Self {
            spec,
            tiles: BTreeMap::new(),
        }
    }

    /// The encoded tile for `name`, rendering it from `source` on first use.
    pub fn get(&mut self, name: &AssetName, source: &[u8]) -> Result<&[u8], RenderError> {
        if !self.tiles.contains_key(name) {
            let tile = render(self.spec, name, source)?;
            self.tiles.insert(name.clone(), tile);
        }
        Ok(self
            .tiles
            .get(name)
            .expect("the tile was just inserted")
            .as_slice())
    }

    /// Forgets every tile, so the next [`TileCache::get`] re-reads each picture.
    ///
    /// Used when the library changed under the cache: the user dropped a file
    /// in, replaced one by hand, or asked to look again. A cache that kept
    /// serving the old bytes would make a replaced picture look like it never
    /// changed.
    pub fn clear(&mut self) {
        self.tiles.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat};

    use super::*;

    /// The six-key deck's own image: 72x72, 4 KiB.
    fn spec() -> ImageSpec {
        ImageSpec::new(72, 72, 4_096, deck_api::ImageFormat::Jpeg)
    }

    fn name() -> AssetName {
        AssetName::parse("live.png").expect("a valid name")
    }

    /// A PNG whose pixels come from `pixel(x, y)`.
    fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut picture = RgbImage::new(width, height);
        for (x, y, colour) in picture.enumerate_pixels_mut() {
            *colour = Rgb(pixel(x, y));
        }
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(picture)
            .write_to(&mut out, ImageFormat::Png)
            .expect("encode the test picture");
        out.into_inner()
    }

    /// Decodes a JPEG back into pixels, so a test can look at what was encoded.
    fn decode(jpeg: &[u8]) -> RgbImage {
        image::load_from_memory(jpeg)
            .expect("the tile is a readable JPEG")
            .to_rgb8()
    }

    #[test]
    fn a_square_picture_fills_the_tile_with_no_black_bands() {
        let source = png(72, 72, |_, _| [0xFF, 0x00, 0x00]);
        let canvas = fit(spec(), &decode(&png(72, 72, |_, _| [0xFF, 0x00, 0x00])));

        assert_eq!(canvas.dimensions(), (72, 72));
        let corner = canvas.get_pixel(0, 0).0;
        assert!(
            corner[0] > 200 && corner[1] < 60 && corner[2] < 60,
            "the picture must reach the corner, got {corner:?}"
        );
        let _ = source;
    }

    #[test]
    fn a_wide_picture_keeps_its_ratio_and_is_centred_on_black() {
        // 4:1, so at 72 wide it is 18 tall and leaves 27 black rows above and
        // below.
        let picture = decode(&png(144, 36, |_, _| [0x00, 0xFF, 0x00]));
        let canvas = fit(spec(), &picture);

        assert_eq!(canvas.dimensions(), (72, 72));
        let top = canvas.get_pixel(36, 0).0;
        assert_eq!(top, [0, 0, 0], "the band above the picture is black");
        let middle = canvas.get_pixel(36, 36).0;
        assert!(
            middle[1] > 200 && middle[0] < 60,
            "the picture keeps its own colour, got {middle:?}"
        );
    }

    #[test]
    fn a_tall_picture_keeps_its_ratio_and_is_centred_on_black() {
        let picture = decode(&png(18, 72, |_, _| [0x00, 0x00, 0xFF]));
        let canvas = fit(spec(), &picture);

        assert_eq!(canvas.dimensions(), (72, 72));
        assert_eq!(canvas.get_pixel(0, 36).0, [0, 0, 0], "left band is black");
        assert_eq!(canvas.get_pixel(71, 36).0, [0, 0, 0], "right band is black");
        let middle = canvas.get_pixel(36, 36).0;
        assert!(middle[2] > 200, "the picture is still blue, got {middle:?}");
    }

    #[test]
    fn a_picture_smaller_than_the_tile_is_scaled_up_rather_than_left_in_a_corner() {
        let picture = decode(&png(16, 16, |_, _| [0xFF, 0xFF, 0xFF]));
        let canvas = fit(spec(), &picture);

        assert_eq!(canvas.dimensions(), (72, 72));
        let corner = canvas.get_pixel(2, 2).0;
        assert!(
            corner.iter().all(|channel| *channel > 200),
            "a small picture still fills the tile, got {corner:?}"
        );
    }

    #[test]
    fn the_encoded_tile_is_the_decks_size_and_within_its_limit() {
        let source = png(200, 50, |x, _| [(x % 256) as u8, 0x40, 0x80]);
        let tile = render(spec(), &name(), &source).expect("a tile");

        assert!(tile.len() <= spec().max_bytes(), "{} bytes", tile.len());
        assert_eq!(decode(&tile).dimensions(), (72, 72));
    }

    #[test]
    fn a_lower_quality_is_tried_before_a_picture_is_refused() {
        // Noise, so the quality actually changes the size.
        let source = png(72, 72, |x, y| {
            let seed = x.wrapping_mul(31) ^ y.wrapping_mul(17);
            [
                (seed % 256) as u8,
                (seed.wrapping_mul(3) % 256) as u8,
                (seed % 97) as u8,
            ]
        });

        let loose = ImageSpec::new(72, 72, usize::MAX, deck_api::ImageFormat::Jpeg);
        let best = render_with_ladder(loose, &name(), &source, &[90]).expect("at 90");
        let floor = render_with_ladder(loose, &name(), &source, &[10]).expect("at 10");
        assert!(
            floor.len() < best.len(),
            "quality 10 ({}) should be smaller than 90 ({})",
            floor.len(),
            best.len()
        );

        // A limit that the top quality misses and the floor meets.
        let tight = ImageSpec::new(72, 72, floor.len() + 8, deck_api::ImageFormat::Jpeg);
        let tile = render_with_ladder(tight, &name(), &source, &[90, 10]).expect("the floor fits");
        assert!(tile.len() <= tight.max_bytes());
        assert_eq!(tile.len(), floor.len(), "the floor encoding was used");
    }

    #[test]
    fn a_tile_with_room_left_is_left_at_the_top_quality() {
        // The deck's own size and limit, and a picture that is far from noisy:
        // the ladder must spend the bytes the deck allows instead of stopping
        // at a rung that was chosen for flat colours.
        let spec = ImageSpec::new(100, 100, 9_000, deck_api::ImageFormat::Jpeg);
        let source = png(100, 100, |x, y| {
            [(x * 2) as u8, (y * 2) as u8, ((x + y) % 256) as u8]
        });

        let tile = render(spec, &name(), &source).expect("a tile");
        let at_top = render_with_ladder(spec, &name(), &source, &[QUALITY_LADDER[0]])
            .expect("the top rung fits in nine kilobytes");

        assert_eq!(QUALITY_LADDER[0], 95);
        assert!(
            tile.len() <= spec.max_bytes(),
            "the tile still has to fit: {} bytes",
            tile.len()
        );
        assert_eq!(
            tile, at_top,
            "a tile that fits at the top quality must not be encoded below it"
        );
    }

    #[test]
    fn a_picture_that_never_fits_is_refused_naming_both_sizes() {
        let source = png(72, 72, |x, y| {
            [(x * y % 256) as u8, (x % 256) as u8, (y % 256) as u8]
        });
        let tiny = ImageSpec::new(72, 72, 64, deck_api::ImageFormat::Jpeg);

        let error = render_with_ladder(tiny, &name(), &source, &[90, 10])
            .expect_err("64 bytes cannot hold a 72x72 photograph");

        let RenderError::TooLarge {
            name: reported,
            bytes,
            max_bytes,
        } = &error
        else {
            panic!("expected a size refusal, got a different error");
        };
        assert_eq!(reported, "live.png");
        assert_eq!(*max_bytes, 64);
        assert!(*bytes > 64, "the reported size must exceed the limit");
        assert_eq!(
            error.to_string(),
            format!(
                "live.png is {bytes} bytes at the lowest quality, and this deck accepts at most 64"
            )
        );
    }

    #[test]
    fn bytes_that_are_not_a_picture_are_refused_as_a_decode_failure() {
        let error = render(spec(), &name(), b"not a png").expect_err("text is not an image");
        let RenderError::Decode { name: reported, .. } = &error else {
            panic!("expected a decode failure, got a different error");
        };
        assert_eq!(reported, "live.png");
        assert!(
            error
                .to_string()
                .starts_with("live.png is not a readable image"),
            "{error}"
        );
    }

    #[test]
    fn a_deck_that_asks_for_no_pixels_is_refused_before_decoding() {
        let empty = ImageSpec::new(0, 72, 4_096, deck_api::ImageFormat::Jpeg);
        let error = render(empty, &name(), b"not a png").expect_err("no pixels");
        assert!(matches!(
            error,
            RenderError::EmptySpec {
                width: 0,
                height: 72
            }
        ));
    }

    #[test]
    fn a_tile_is_rendered_once_and_then_served_from_the_cache() {
        let source = png(72, 72, |_, _| [0x10, 0x20, 0x30]);
        let mut cache = TileCache::new(spec());

        let first = cache.get(&name(), &source).expect("render").to_vec();
        assert_eq!(cache.tiles.len(), 1);

        // The second call gets the same bytes even though the source is now
        // nonsense: a cached tile is not re-decoded.
        let second = cache
            .get(&name(), b"no longer an image")
            .expect("from the cache")
            .to_vec();
        assert_eq!(first, second);
    }

    #[test]
    fn clearing_the_cache_makes_the_next_read_see_the_new_file() {
        let red = png(72, 72, |_, _| [0xFF, 0x00, 0x00]);
        let blue = png(72, 72, |_, _| [0x00, 0x00, 0xFF]);
        let mut cache = TileCache::new(spec());

        let before = cache.get(&name(), &red).expect("render").to_vec();
        cache.clear();

        let after = cache.get(&name(), &blue).expect("render again").to_vec();
        assert_ne!(before, after, "the cleared tile was read again");
    }

    #[test]
    fn a_preview_is_the_fitted_tile_in_bgra() {
        // 4:1 red, so it fills the width and leaves black bands above and below.
        // Red (0xFF, 0x00, 0x00) in RGB is (0x00, 0x00, 0xFF) in the BGRA the
        // renderer reads, which is what the window shows.
        let source = png(144, 36, |_, _| [0xFF, 0x00, 0x00]);
        let frame = preview_frame(spec(), &name(), &source).expect("a preview");

        assert_eq!(frame.buffer().dimensions(), (72, 72));

        // The bands are the fitted result: the picture never reaches row 0.
        assert_eq!(
            frame.buffer().get_pixel(36, 0).0,
            [0, 0, 0, 255],
            "the band above the picture is black"
        );

        // The picture itself is red, so its bytes are blue-first. A preview in
        // the wrong channel order is the difference the user sees as "the
        // colours are swapped", which is why this is asserted and not eyeballed.
        assert_eq!(
            frame.buffer().get_pixel(36, 36).0,
            [0, 0, 0xFF, 0xFF],
            "red reaches the window as the renderer's BGRA red"
        );
    }

    #[test]
    fn a_preview_keeps_the_picture_the_way_the_deck_shows_it() {
        // A mark in the top-left corner of the source: the payload is flipped on
        // the way to the deck, which flips it back, so the glass shows the mark
        // in the top-left. A preview that flipped it too would disagree with the
        // key in front of the user, which is exactly what a preview is for.
        let source = png(72, 72, |x, y| {
            if x < 12 && y < 12 {
                [0xFF, 0xFF, 0xFF]
            } else {
                [0x00, 0x00, 0x00]
            }
        });
        let frame = preview_frame(spec(), &name(), &source).expect("a preview");

        let corner = frame.buffer().get_pixel(2, 2).0;
        assert_eq!(
            corner,
            [0xFF, 0xFF, 0xFF, 0xFF],
            "the mark stays in the corner the source put it in"
        );
        let opposite = frame.buffer().get_pixel(69, 69).0;
        assert_eq!(
            opposite,
            [0, 0, 0, 0xFF],
            "the rest of the tile is the picture's own dark ground"
        );
    }

    #[test]
    fn a_picture_too_big_to_send_can_still_be_previewed() {
        let source = png(72, 72, |x, y| {
            [(x * y % 256) as u8, (x % 256) as u8, (y % 256) as u8]
        });
        let tiny = ImageSpec::new(72, 72, 64, deck_api::ImageFormat::Jpeg);

        assert!(
            render(tiny, &name(), &source).is_err(),
            "64 bytes cannot hold this tile"
        );
        let frame = preview_frame(tiny, &name(), &source).expect("a preview is not a payload");
        assert_eq!(frame.buffer().dimensions(), (72, 72));
    }

    #[test]
    fn a_preview_refuses_bytes_that_are_not_a_picture() {
        let error = preview_frame(spec(), &name(), b"not a png")
            .err()
            .expect("text is not an image");
        assert!(matches!(error, RenderError::Decode { .. }), "{error}");
    }

    /// An RGBA picture whose pixels come from `pixel(x, y)`.
    fn rgba(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> RgbaImage {
        let mut picture = RgbaImage::new(width, height);
        for (x, y, colour) in picture.enumerate_pixels_mut() {
            *colour = Rgba(pixel(x, y));
        }
        picture
    }

    #[test]
    fn a_transparent_pixel_is_flattened_onto_black() {
        // Half-transparent white is the grey the multiply produces, and a
        // pixel that is not there at all is the black a JPEG can hold.
        let picture = rgba(2, 1, |x, _| {
            if x == 0 {
                [0xFF, 0xFF, 0xFF, 0x80]
            } else {
                [0xFF, 0xFF, 0xFF, 0x00]
            }
        });
        let flat = on_black(&picture);

        assert_eq!(flat.get_pixel(0, 0).0, [0x80, 0x80, 0x80]);
        assert_eq!(flat.get_pixel(1, 0).0, [0, 0, 0]);
    }

    #[test]
    fn an_icon_goes_through_the_same_pipeline_as_a_picture() {
        // The same pixels, once as an image file and once as the pixels
        // Windows handed over: the payload must be byte for byte the same, or
        // a key would show one thing and send another.
        let pixels = rgba(32, 32, |x, y| [(x * 8) as u8, (y * 8) as u8, 0x40, 0xFF]);
        let source = png(32, 32, |x, y| [(x * 8) as u8, (y * 8) as u8, 0x40]);

        let from_pixels = render_rgba(spec(), "app.exe", &pixels).expect("a tile");
        let from_file = render(spec(), &name(), &source).expect("a tile");

        assert_eq!(from_pixels, from_file);
        assert!(from_pixels.len() <= spec().max_bytes());
    }

    #[test]
    fn a_preview_from_pixels_is_the_fitted_tile_the_deck_would_show() {
        // A 4:1 picture, so the fit leaves black bands above and below, exactly
        // as it does for a picture that came out of a file.
        let pixels = rgba(144, 36, |_, _| [0xFF, 0x00, 0x00, 0xFF]);
        let frame = preview_frame_rgba(spec(), &pixels).expect("a preview");

        assert_eq!(frame.buffer().dimensions(), (72, 72));
        assert_eq!(frame.buffer().get_pixel(36, 0).0, [0, 0, 0, 255]);
        assert_eq!(
            frame.buffer().get_pixel(36, 36).0,
            [0, 0, 0xFF, 0xFF],
            "the picture reaches the window in the renderer's BGRA"
        );
    }

    #[test]
    fn pixels_with_no_deck_to_size_them_keep_their_own_size() {
        // Before a deck answers there is no image size to fit to, and the window
        // still has to show what the key runs.
        let pixels = rgba(32, 32, |_, _| [0x00, 0xFF, 0x00, 0xFF]);
        let frame = frame_from_rgba(&pixels);

        assert_eq!(frame.buffer().dimensions(), (32, 32));
        assert_eq!(frame.buffer().get_pixel(4, 4).0, [0x00, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn pixels_are_refused_for_a_deck_that_asks_for_no_pixels() {
        let empty = ImageSpec::new(72, 0, 4_096, deck_api::ImageFormat::Jpeg);
        let pixels = rgba(32, 32, |_, _| [0xFF, 0xFF, 0xFF, 0xFF]);

        assert!(matches!(
            render_rgba(empty, "app.exe", &pixels),
            Err(RenderError::EmptySpec { height: 0, .. })
        ));
        assert!(matches!(
            preview_frame_rgba(empty, &pixels),
            Err(RenderError::EmptySpec { height: 0, .. })
        ));
    }
}

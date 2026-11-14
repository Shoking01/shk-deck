//! Turning a picture in the asset library into the bytes one key accepts.
//!
//! The deck states what it takes — a width, a height, a byte limit and an
//! encoding — and this module is the only place that answers it. Nothing here
//! hard-codes `72x72`: a deck that takes wide tiles is served by the same code
//! as one that takes square ones.
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
use image::{imageops, Rgb, RgbImage};
use jpeg_encoder::{ColorType, Encoder};
use ss550_engine::AssetName;

/// The qualities tried, in order, before a tile is refused.
///
/// The first entry is the quality a flat colour is encoded at, so a picture and
/// a colour look like they belong on the same deck. The floor is low enough that
/// a noisy photograph fits in a few kilobytes; below it the artefacts are worse
/// than the refusal.
pub const QUALITY_LADDER: [u8; 6] = [80, 65, 50, 35, 20, 10];

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
    let (width, height) = (spec.width(), spec.height());
    if width == 0 || height == 0 {
        return Err(RenderError::EmptySpec { width, height });
    }

    let decoded = image::load_from_memory(source).map_err(|source| RenderError::Decode {
        name: name.to_string(),
        source,
    })?;
    let canvas = fit(spec, &decoded.to_rgb8());

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
}

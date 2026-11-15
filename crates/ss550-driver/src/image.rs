//! Image handling for SS550 button displays.

use crate::error::Ss550Error;
use std::io::Cursor;

/// Maximum JPEG payload size for a single button (9000 bytes per the app docs).
pub const MAX_JPEG_SIZE: usize = 9_000;

/// The panel's own pixel size, which is what a key image has to cover.
///
/// The community reference for this family sizes button images at 100×100 and
/// sends exactly that. On the deck in front of this project a 100×100 payload
/// does not reach the panel's top and left edges: a measured 7.5 % of the panel
/// on each keeps whatever the key showed before, which is invisible on a solid
/// colour and a visible frame around a picture. The panel measures 108 pixels
/// across (100 / (1 − 0.075)), and that is what the encoder is asked for, so the
/// picture fills the key the deck actually has.
pub const WIDTH: u16 = 108;
/// See [`WIDTH`].
pub const HEIGHT: u16 = 108;

/// Rotation options for button images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    Rot0,
    Rot90,
    #[default]
    Rot180,
    Rot270,
}

/// A button image ready to send to the device.
#[derive(Debug, Clone)]
pub struct ButtonImage {
    /// The JPEG payload bytes.
    pub jpeg: Vec<u8>,
    /// Target key index (0-14).
    pub key: u8,
}

impl ButtonImage {
    /// Creates a red test image for debugging.
    pub fn red(key: u8) -> Result<Self, Ss550Error> {
        let mut rgb = vec![0u8; (WIDTH as usize) * (HEIGHT as usize) * 3];
        for px in rgb.chunks_exact_mut(3) {
            px[0] = 255;
            px[1] = 0;
            px[2] = 0;
        }
        Self::from_rgb(key, &rgb)
    }

    /// Creates a white test image for debugging.
    pub fn white(key: u8) -> Result<Self, Ss550Error> {
        let rgb = vec![0xffu8; (WIDTH as usize) * (HEIGHT as usize) * 3];
        Self::from_rgb(key, &rgb)
    }

    /// Creates a button image from raw RGB data.
    pub fn from_rgb(key: u8, rgb: &[u8]) -> Result<Self, Ss550Error> {
        let width = WIDTH as usize;
        let height = HEIGHT as usize;
        if rgb.len() != width * height * 3 {
            return Err(Ss550Error::ImageEncodeError(format!(
                "RGB data length {} does not match {}x{}x3",
                rgb.len(),
                width,
                height
            )));
        }

        let mut jpeg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpeg, 90)
            .encode(rgb, WIDTH, HEIGHT, jpeg_encoder::ColorType::Rgb)
            .map_err(|e| Ss550Error::ImageEncodeError(e.to_string()))?;

        if jpeg.len() > MAX_JPEG_SIZE {
            return Err(Ss550Error::PayloadTooLarge {
                max: MAX_JPEG_SIZE,
                size: jpeg.len(),
            });
        }

        Ok(Self { jpeg, key })
    }

    /// Loads a JPEG image from memory.
    pub fn from_jpeg(data: &[u8], key: u8) -> Result<Self, Ss550Error> {
        let reader = image::ImageReader::new(Cursor::new(data));
        let img = reader
            .decode()
            .map_err(|e| Ss550Error::ImageDecodeError(e.to_string()))?;
        let rgb = img.into_rgb8();
        Self::from_rgb(key, &rgb)
    }
}

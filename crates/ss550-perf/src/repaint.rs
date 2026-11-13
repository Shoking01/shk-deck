//! The work a full repaint costs: encode every tile, then send it.
//!
//! A profile switch arrives in M6, but the expensive part of one is already
//! here: every key on the deck gets a new image, which means encoding one JPEG
//! per key at the deck's own spec and pushing all of them over the wire. That is
//! exactly what `PAINT ALL` does today, so this measures the real work instead
//! of waiting for the feature that will call it.
//!
//! **Nothing here touches hardware.** The images go through
//! `deck_simulator::Ss550WireFake`, the same transport the driver's tests use,
//! so the number is the driver's own upload path without a device attached.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deck_api::{Deck, KeyId, Transport, TransportError};
use deck_simulator::Ss550WireFake;
use ss550_driver::Ss550Deck;

use crate::stats::Summary;

/// The fake behind a lock, so the deck can own its transport.
#[derive(Clone)]
struct SharedWire {
    inner: Arc<Mutex<Ss550WireFake>>,
}

impl Transport for SharedWire {
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .write_report(bytes)
    }

    fn read_input(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .read_input(buf, timeout)
    }

    fn read_control(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .read_control(buf)
    }
}

/// A solid-colour JPEG of the given size, as the window hands one over.
fn solid_jpeg(width: u16, height: u16) -> Result<Vec<u8>, String> {
    let rgb = vec![0x20_u8; usize::from(width) * usize::from(height) * 3];
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 75)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .map_err(|error| format!("the probe's own image did not encode: {error}"))?;
    Ok(jpeg)
}

/// What one full repaint cost.
pub struct Report {
    /// How many keys the deck has.
    pub keys: u16,
    /// The size of one encoded tile, in bytes.
    pub tile_bytes: usize,
    /// Encoding every tile.
    pub encode: Summary,
    /// Sending every tile over the wire.
    pub send: Summary,
    /// Encoding and sending, per repaint.
    pub total: Summary,
}

/// Whether this build is optimised.
///
/// A debug build encodes an image roughly thirteen times slower than an
/// optimised one on the same machine: the encoder's arithmetic is only
/// optimised with `--release`. A repaint number from a debug build is therefore
/// not the repaint the user gets, and this is how the tool says so.
pub fn is_release() -> bool {
    !cfg!(debug_assertions)
}

/// Measures `iterations` full repaints of the deck behind the wire fake.
pub fn measure(iterations: usize) -> Result<Report, String> {
    if iterations == 0 {
        return Err("iterations must be at least 1".to_string());
    }

    let wire = SharedWire {
        inner: Arc::new(Mutex::new(Ss550WireFake::new())),
    };
    let mut deck = Ss550Deck::over_transport(Box::new(wire));
    let spec = deck.image_spec();
    let keys = deck.geometry().key_count();

    let mut encode = Vec::with_capacity(iterations);
    let mut send = Vec::with_capacity(iterations);
    let mut total = Vec::with_capacity(iterations);
    let mut tile_bytes = 0;

    for _ in 0..iterations {
        let started = Instant::now();
        let mut tiles = Vec::with_capacity(usize::from(keys));
        for _ in 0..keys {
            tiles.push(solid_jpeg(spec.width(), spec.height())?);
        }
        encode.push(started.elapsed());
        tile_bytes = tiles.first().map_or(0, Vec::len);

        let started = Instant::now();
        for (index, tile) in tiles.iter().enumerate() {
            let key = KeyId::new(u16::try_from(index).map_err(|_| "key index too large")?);
            deck.set_key_image(key, tile)
                .map_err(|error| format!("the deck refused key {index}: {error}"))?;
        }
        send.push(started.elapsed());

        let repaint =
            encode.last().copied().unwrap_or_default() + send.last().copied().unwrap_or_default();
        total.push(repaint);
    }

    Ok(Report {
        keys,
        tile_bytes,
        encode: Summary::of(&encode).ok_or("no encode samples")?,
        send: Summary::of(&send).ok_or("no send samples")?,
        total: Summary::of(&total).ok_or("no repaint samples")?,
    })
}

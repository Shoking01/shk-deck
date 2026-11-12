//! The SS550 as a [`Deck`]: the model, expressed as the seam's data and errors.
//!
//! This is the only place the deck's own constants — key count, grid, image
//! size, payload limit, brightness range — are stated to the code above the
//! seam. Everything here is either a fact the driver already owns or a typed
//! refusal; nothing is forwarded blindly, and nothing is faked.
//!
//! Two rules shape this file:
//!
//! - **The cheap check comes first.** A key is checked before the payload, and
//!   the payload length before the payload's contents. A deck that decodes an
//!   image to answer a question the key already settled spends work on a
//!   question it could answer for free, and on hardware that work is a write
//!   waiting to happen.
//! - **Refuse, never clamp or pretend.** Brightness past the range is refused
//!   by the seam before the driver's clamp can see it, and the three
//!   capabilities the verified registry forbids — `clear_key`, `sleep` and the
//!   firmware identity — are declared unsupported instead of being accepted and
//!   quietly doing nothing.

use std::io::Cursor;
use std::time::Duration;

use deck_api::{
    ButtonState as SeamButtonState, Capabilities, Deck, DeckError, DeckOperation, Geometry,
    ImageFormat, ImageSpec, KeyEvent, KeyId, Transport, TransportError, VALUE_MAX, VALUE_MIN,
};

use crate::connection::{ButtonEvent, Device};
use crate::error::Ss550Error;
use crate::image::{ButtonImage, HEIGHT, MAX_JPEG_SIZE, WIDTH};
use crate::keys;

/// Keys per row on the SS550's 3×5 grid.
const COLUMNS: u16 = 5;

/// The deck's key count, in the seam's unit.
const KEY_COUNT: u16 = keys::KEY_COUNT as u16;

/// The SS550 v3, behind the [`Deck`] seam.
///
/// Holds the driver's [`Device`] and states what it can do. A caller that only
/// knows "a deck" holds a [`DeckHandle`](deck_api::DeckHandle) built from this
/// type; a caller that knows it has an SS550 can use [`Ss550Deck::connect`].
///
/// # Examples
///
/// ```no_run
/// use deck_api::Deck;
/// use ss550_driver::Ss550Deck;
///
/// let mut deck = Ss550Deck::connect()?;
/// deck.set_brightness(60)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Ss550Deck {
    device: Device,
}

impl Ss550Deck {
    /// Connects to a deck once its boot sequence has settled.
    ///
    /// Same rules as [`Device::connect`]: the deck re-enumerates while it
    /// boots, so this waits for a settled presence before opening a handle.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ss550_driver::Ss550Deck;
    ///
    /// let deck = Ss550Deck::connect()?;
    /// # Ok::<(), ss550_driver::Ss550Error>(())
    /// ```
    pub fn connect() -> Result<Self, Ss550Error> {
        Device::connect().map(Self::from_device)
    }

    /// Connects, waiting up to `timeout` for the deck's boot to settle.
    ///
    /// Use [`LONG_CONNECT_TIMEOUT`](crate::LONG_CONNECT_TIMEOUT) for a
    /// long-lived client that must win the deck's boot window on the first try.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ss550_driver::{Ss550Deck, LONG_CONNECT_TIMEOUT};
    ///
    /// let deck = Ss550Deck::connect_with_timeout(LONG_CONNECT_TIMEOUT)?;
    /// # Ok::<(), ss550_driver::Ss550Error>(())
    /// ```
    pub fn connect_with_timeout(timeout: Duration) -> Result<Self, Ss550Error> {
        Device::connect_with_timeout(timeout).map(Self::from_device)
    }

    /// Wraps a caller-supplied transport instead of a device.
    ///
    /// This is the seam made usable without hardware: the tests and the
    /// simulator drive the real driver through it, and the verified-command
    /// gate still runs on every write — a transport cannot make an unproven
    /// command sendable, and every byte still leaves through
    /// [`Device`]'s one gated funnel.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use deck_api::{Deck, Transport, TransportError};
    /// use ss550_driver::Ss550Deck;
    ///
    /// /// A transport that keeps the bytes instead of sending them.
    /// struct InMemory {
    ///     written: usize,
    /// }
    ///
    /// impl Transport for InMemory {
    ///     fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError> {
    ///         self.written += 1;
    ///         Ok(bytes.len())
    ///     }
    ///
    ///     fn read_input(
    ///         &mut self,
    ///         _buf: &mut [u8],
    ///         _timeout: Duration,
    ///     ) -> Result<usize, TransportError> {
    ///         Ok(0)
    ///     }
    ///
    ///     fn read_control(&mut self, _buf: &mut [u8]) -> Result<usize, TransportError> {
    ///         Ok(0)
    ///     }
    /// }
    ///
    /// let deck = Ss550Deck::over_transport(Box::new(InMemory { written: 0 }));
    /// assert_eq!(deck.geometry().key_count(), 15);
    /// ```
    pub fn over_transport(transport: Box<dyn Transport + Send>) -> Self {
        Self::from_device(Device::over_transport(transport))
    }

    fn from_device(device: Device) -> Self {
        Self { device }
    }

    /// Wakes the deck before an operation that needs it awake.
    ///
    /// The seam has no `initialize`: a caller that had to wake the deck before
    /// painting would be coding against this model, not against a deck. The
    /// driver's `initialize` is idempotent, so this writes `DIS` exactly once,
    /// before the first paint or clear, and nothing afterwards.
    fn ensure_awake(&mut self, operation: DeckOperation) -> Result<(), DeckError> {
        self.device
            .initialize()
            .map_err(|error| seam_error(operation, error))
    }
}

impl Deck for Ss550Deck {
    fn geometry(&self) -> Geometry {
        Geometry::new(KEY_COUNT, COLUMNS)
    }

    fn image_spec(&self) -> ImageSpec {
        ImageSpec::new(WIDTH, HEIGHT, MAX_JPEG_SIZE, ImageFormat::Jpeg)
    }

    fn capabilities(&self) -> Capabilities {
        // `clear_key` and `sleep` are `unverified` in the protocol registry, so
        // they are not capabilities of this deck until a test deck proves them;
        // `CLE` with `0xFF` and `LIG` were proven live. The SS550 has no dials.
        Capabilities::new()
            .with_brightness(true)
            .with_clear_all(true)
            .with_clear_key(false)
            .with_sleep(false)
            .with_dials(0)
    }

    fn identity(&self) -> Result<String, DeckError> {
        // `VER` is `unverified`, and writing it halts the USB endpoint
        // (`docs/troubleshooting/deck-stuck.md`). The static identity report is
        // informational and proves nothing about the firmware, so returning it
        // would be a fabricated identity; returning an empty string would be a
        // silent failure. The seam gets the typed refusal instead.
        Err(DeckError::Unsupported {
            operation: DeckOperation::Identity,
        })
    }

    fn set_brightness(&mut self, value: u8) -> Result<(), DeckError> {
        // Before the driver's clamp: a clamped request is a request that
        // changed without telling the caller.
        if !(VALUE_MIN..=VALUE_MAX).contains(&value) {
            return Err(DeckError::value_out_of_range(
                DeckOperation::Brightness,
                value,
            ));
        }

        self.ensure_awake(DeckOperation::Brightness)?;
        self.device
            .set_brightness(value)
            .map_err(|error| seam_error(DeckOperation::Brightness, error))
    }

    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
        let geometry = self.geometry();
        if !geometry.contains(key) {
            return Err(DeckError::KeyOutOfRange {
                key,
                key_count: geometry.key_count(),
            });
        }

        let spec = self.image_spec();
        if jpeg.len() > spec.max_bytes() {
            return Err(DeckError::ImageTooLarge {
                bytes: jpeg.len(),
                max_bytes: spec.max_bytes(),
            });
        }

        // Only now is the payload itself worth reading: the key and the length
        // answered both cheap questions already.
        check_image_shape(jpeg, spec)?;

        let index = u8::try_from(key.index()).map_err(|_| DeckError::KeyOutOfRange {
            key,
            key_count: geometry.key_count(),
        })?;

        self.ensure_awake(DeckOperation::SetKeyImage)?;
        self.device
            .set_key_image(ButtonImage {
                jpeg: jpeg.to_vec(),
                key: index,
            })
            .map_err(|error| seam_error(DeckOperation::SetKeyImage, error))
    }

    fn clear_key(&mut self, _key: KeyId) -> Result<(), DeckError> {
        // `CLE` with a key is `unverified`: the live clear-per-key check is
        // still pending, so the gate would refuse the write anyway. Saying so
        // here makes the refusal typed instead of an I/O error.
        Err(DeckError::Unsupported {
            operation: DeckOperation::ClearKey,
        })
    }

    fn clear_all(&mut self) -> Result<(), DeckError> {
        self.ensure_awake(DeckOperation::ClearAll)?;
        self.device
            .clear_all()
            .map_err(|error| seam_error(DeckOperation::ClearAll, error))
    }

    fn sleep(&mut self) -> Result<(), DeckError> {
        // `HAN` is `unverified`: encoded from the reference implementation and
        // never exercised on a deck.
        Err(DeckError::Unsupported {
            operation: DeckOperation::Sleep,
        })
    }

    fn set_dial(&mut self, dial: u8, _value: u8) -> Result<(), DeckError> {
        // No dials: every dial is out of range, and the capability is the one
        // place that count lives.
        Err(DeckError::DialOutOfRange {
            dial,
            dial_count: self.capabilities().dial_count(),
        })
    }

    fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
        let events = self
            .device
            .poll_events(timeout)
            .map_err(|error| seam_error(DeckOperation::PollEvents, error))?;
        Ok(events.into_iter().map(seam_event).collect())
    }

    fn reconnect(&mut self) -> Result<(), DeckError> {
        self.device
            .reconnect()
            .map_err(|error| seam_error(DeckOperation::Reconnect, error))
    }
}

impl std::fmt::Debug for Ss550Deck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ss550Deck")
            .field("geometry", &self.geometry())
            .field("device", &self.device)
            .finish_non_exhaustive()
    }
}

/// Refuses a payload that is not the image the deck takes.
///
/// Only the header is read: the pixels are never decoded and the caller's bytes
/// are forwarded unchanged, so what the caller encoded is what the deck
/// receives. A payload whose size cannot be read at all is refused as `0x0` —
/// this deck has no way to paint an image it cannot measure.
fn check_image_shape(jpeg: &[u8], spec: ImageSpec) -> Result<(), DeckError> {
    let dimensions = image::ImageReader::new(Cursor::new(jpeg))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());

    let refusal = |width: u32, height: u32| DeckError::ImageWrongSize {
        width: u16::try_from(width).unwrap_or(u16::MAX),
        height: u16::try_from(height).unwrap_or(u16::MAX),
        expected_width: spec.width(),
        expected_height: spec.height(),
    };

    match dimensions {
        Some((width, height))
            if width == u32::from(spec.width()) && height == u32::from(spec.height()) =>
        {
            Ok(())
        }
        Some((width, height)) => Err(refusal(width, height)),
        None => Err(refusal(0, 0)),
    }
}

/// Maps one driver edge onto the seam's event.
fn seam_event(event: ButtonEvent) -> KeyEvent {
    let state = match event.state {
        crate::ButtonState::Pressed => SeamButtonState::Pressed,
        crate::ButtonState::Released => SeamButtonState::Released,
    };
    KeyEvent::new(KeyId::new(u16::from(event.key.to_index())), state)
}

/// Maps a driver failure onto the seam's typed errors.
///
/// `operation` is what the caller asked for, so a command the registry forbids
/// becomes [`DeckError::Unsupported`] naming the operation instead of the raw
/// string the driver carries. Everything else reaching this point is the wire:
/// the adapter has already refused the key, the length, the shape and the value
/// range itself.
fn seam_error(operation: DeckOperation, error: Ss550Error) -> DeckError {
    match error {
        Ss550Error::CommandNotVerified { .. } => DeckError::Unsupported { operation },
        Ss550Error::DeviceNotFound { .. } => DeckError::DeviceNotFound,
        Ss550Error::TransportStalled { detail } => {
            DeckError::Transport(TransportError::Stalled(detail))
        }
        other => DeckError::Transport(TransportError::Io(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::check_image_shape;
    use deck_api::{DeckError, ImageFormat, ImageSpec};

    fn spec() -> ImageSpec {
        ImageSpec::new(100, 100, 9_000, ImageFormat::Jpeg)
    }

    /// Encodes a JPEG of the given size, so the shape check has real bytes.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let rgb = vec![0x40_u8; usize::from(width) * usize::from(height) * 3];
        let mut bytes = Vec::new();
        jpeg_encoder::Encoder::new(&mut bytes, 90)
            .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
            .expect("a synthetic image encodes");
        bytes
    }

    #[test]
    fn an_image_of_the_declared_size_is_accepted() {
        assert_eq!(check_image_shape(&jpeg(100, 100), spec()), Ok(()));
    }

    #[test]
    fn an_image_of_another_size_is_refused_with_its_own_numbers() {
        assert_eq!(
            check_image_shape(&jpeg(72, 72), spec()),
            Err(DeckError::ImageWrongSize {
                width: 72,
                height: 72,
                expected_width: 100,
                expected_height: 100,
            })
        );
    }

    #[test]
    fn a_payload_that_is_not_an_image_is_refused_rather_than_sent() {
        // A run of bytes is not a 100x100 image, so it must not reach the deck
        // just because it is short enough to fit.
        assert_eq!(
            check_image_shape(&[0_u8; 64], spec()),
            Err(DeckError::ImageWrongSize {
                width: 0,
                height: 0,
                expected_width: 100,
                expected_height: 100,
            })
        );
    }
}

//! Pure SS550 HID pack builders (byte-exact to `mirajazz`).
//!
//! Wire layout reminder (wet = report incl. report ID at `[0]`):
//!
//! ```text
//! [0]=0x00, [1..4]="CRT", [4]=0, [5]=0, [6..]=tag, fields follow tag.
//! Every pack is padded with zeros to 1025 bytes.
//! ```
//!
//! Reference: `4ndv/mirajazz` `src/device.rs` — the exact vectors below are
//! copied from its `initialize`, `set_brightness`, `send_image`,
//! `clear_button_image`, `clear_all_button_images`, `sleep`, `keep_alive`,
//! `flush`, and `set_mode` functions.

use crate::error::ProtocolError;

/// Output report size (1025 = 1 report ID + 1024 payload).
pub const REPORT_SIZE: usize = 1025;

/// Payload size per image chunk.
pub const CHUNK_SIZE: usize = 1024;

/// Maximum image size encodable in a BAT BE16 length field.
pub const MAX_IMAGE_SIZE: usize = 65_535;

/// A single 1025-byte HID output report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack([u8; REPORT_SIZE]);

impl Pack {
    /// Returns the pack as a byte slice (length 1025).
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn blank() -> Self {
        Self([0u8; REPORT_SIZE])
    }

    fn with_crt_header() -> Self {
        let mut p = Self::blank();
        p.0[0] = 0x00;
        p.0[1..4].copy_from_slice(b"CRT");
        // Wire [4] and [5] stay zero — mirajazz emits two variant zeros
        // before the tag on every CRT command.
        p
    }

    fn with_tag(&mut self, tag: [u8; 3]) {
        self.0[6..9].copy_from_slice(&tag);
    }
}

/// Builds the wake pack: `CRT|DIS`.
///
/// ```text
/// [0]=0x00, [1..4]="CRT", [4]=0, [5]=0, [6..9]="DIS"
/// ```
pub fn build_dis() -> Pack {
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"DIS");
    p
}

/// Builds the brightness pack: `CRT|LIG`.
///
/// ```text
/// [6..9]="LIG", [9]=0, [10]=0, [11]=brightness (0-100)
/// ```
pub fn build_lig(brightness: u8) -> Result<Pack, ProtocolError> {
    if brightness > 100 {
        return Err(ProtocolError::InvalidBrightness(brightness));
    }
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"LIG");
    // Wire [9] and [10] stay zero; wire [11] carries the value.
    p.0[11] = brightness;
    Ok(p)
}

/// Builds the plain-image header: `CRT|BAT`, sent **before** the JPEG chunks.
///
/// ```text
/// [6..9]="BAT", [9]=0, [10]=0, [11]=len_hi, [12]=len_lo, [13]=firmware_key
/// ```
///
/// `image_len` is the JPEG byte length; `firmware_key` is the 1-based
/// firmware key (1-15).
pub fn build_bat(image_len: usize, firmware_key: u8) -> Result<Pack, ProtocolError> {
    if !(1..=15).contains(&firmware_key) {
        return Err(ProtocolError::InvalidKey(firmware_key));
    }
    if image_len > MAX_IMAGE_SIZE {
        return Err(ProtocolError::SizeTooLarge(image_len));
    }
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"BAT");
    p.0[11] = (image_len >> 8) as u8;
    p.0[12] = (image_len & 0xFF) as u8;
    p.0[13] = firmware_key;
    Ok(p)
}

/// Builds the flush/commit pack: `CRT|STP`.
///
/// Sent **once**, after every image's chunks. Without it nothing appears.
pub fn build_stp() -> Pack {
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"STP");
    p
}

/// Builds the clear-all pack: `CRT|CLE` with key field `0xFF`.
///
/// ```text
/// [6..9]="CLE", [9]=0, [10]=0, [11]=0, [12]=0xFF
/// ```
pub fn build_cle_all() -> Pack {
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"CLE");
    p.0[12] = 0xFF;
    p
}

/// Builds the clear-one-key pack: `CRT|CLE` for a single 1-based firmware key.
///
/// Wire [12] carries the firmware key (1-15). Use [`build_cle_all`] to
/// clear every key.
pub fn build_cle_key(firmware_key: u8) -> Result<Pack, ProtocolError> {
    if !(1..=15).contains(&firmware_key) {
        return Err(ProtocolError::InvalidKey(firmware_key));
    }
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"CLE");
    p.0[12] = firmware_key;
    Ok(p)
}

/// Builds the firmware-version query: `CRT|VER`.
///
/// Static/recovered from `SDLibrary1.dll` (`sendGetHardwareFirmwareVersion`);
/// pay attention that the streamdock reader only accepts `V1./V2./V3.` for
/// `StreamDock[293]`, so a silent reply does not prove the pack wrong.
pub fn build_ver() -> Pack {
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"VER");
    p
}

/// Builds the bare handshake pack: report ID + `HAN` with **no** `CRT` prefix.
///
/// This is the connect-time `sendHandshakePack` form. The `CRT|HAN` form
/// below is the sleep/keep-alive form — they are not interchangeable.
pub fn build_han_bare() -> Pack {
    let mut p = Pack::blank();
    p.0[0] = 0x00;
    p.0[1..4].copy_from_slice(b"HAN");
    p
}

/// Builds the sleep pack: `CRT|HAN`.
///
/// `mirajazz::Device::sleep` sends `CRT|HAN`; the bare HAN above is the
/// handshake form. Keep the two distinct.
pub fn build_sleep() -> Pack {
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"HAN");
    p
}

/// Builds the keep-alive pack: `CRT|CONNECT`.
///
/// The 7-char tag spans wire `[6..13]` (`CONNECT` overwrites the usual
/// field bytes). Mirajazz `keep_alive` sends this on a timer after connect.
pub fn build_connect() -> Pack {
    let mut p = Pack::with_crt_header();
    // Tag is 7 chars starting at wire 6: "CONNECT" occupies [6..13].
    p.0[6..13].copy_from_slice(b"CONNECT");
    p
}

/// Builds the mode pack: `CRT|MOD` with ASCII digit mode.
///
/// ```text
/// [6..9]="MOD", [9]=0, [10]=0, [11]='0'+mode
/// ```
///
/// Mirajazz `set_mode` sends `0x30 + mode` (ASCII `'0'+mode`). Modes are
/// single digits 0-9.
pub fn build_mod(mode: u8) -> Result<Pack, ProtocolError> {
    if mode > 9 {
        return Err(ProtocolError::InvalidMode(mode));
    }
    let mut p = Pack::with_crt_header();
    p.with_tag(*b"MOD");
    p.0[11] = b'0' + mode;
    Ok(p)
}

/// Builds one JPEG chunk pack: report ID `0x00` + up to 1024 image bytes.
///
/// Zero-padded to 1025. There is **no per-chunk length field** — the device
/// relies on the fixed report size and the BAT length.
pub fn build_image_chunk(jpeg_chunk: &[u8]) -> Result<Pack, ProtocolError> {
    if jpeg_chunk.len() > CHUNK_SIZE {
        return Err(ProtocolError::ChunkTooLarge(jpeg_chunk.len()));
    }
    let mut p = Pack::blank();
    p.0[0] = 0x00;
    p.0[1..1 + jpeg_chunk.len()].copy_from_slice(jpeg_chunk);
    Ok(p)
}

/// Splits JPEG bytes into 1024-byte packs, each padded to 1025.
///
/// Returns an empty vector for empty input (never sends a bare chunk).
pub fn build_image_chunks(jpeg: &[u8]) -> Vec<Pack> {
    jpeg.chunks(CHUNK_SIZE)
        .map(|c| build_image_chunk(c).expect("chunk is at most CHUNK_SIZE"))
        .collect()
}

//! What the fake understood from one output report.
//!
//! The builders in [`ss550_protocol`] are the only authority on how a pack is
//! laid out; this module is their inverse. It reads the offsets the builders
//! write — the `CRT` header, the tag, the field bytes — and names the result
//! with [`ss550_protocol::Command`], so a transcript can never invent a
//! vocabulary of its own.
//!
//! Nothing here decides what a *deck* should do with a command. That is the
//! deck's business; this crate only knows whether the bytes on the wire are a
//! command it recognises.

use ss550_protocol::{Command, REPORT_SIZE};
use thiserror::Error;

/// A command the fake understood, named the way the registry names it.
///
/// The payload-bearing variants carry the field that was on the wire, so a test
/// can assert what was asked for and not merely that something was asked.
///
/// ```
/// use deck_simulator::Ss550Command;
///
/// let requested = Ss550Command::Brightness { value: 30 };
/// assert_eq!(requested.name(), "brightness");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ss550Command {
    /// `CRT|DIS`, the wake pack.
    DisplayOn,
    /// `CRT|LIG`, with the brightness in the pack.
    Brightness {
        /// The brightness the pack asked for.
        value: u8,
    },
    /// An image: `CRT|BAT`, its chunk packs, and the `CRT|STP` that commits it.
    ///
    /// The entry is recorded once, at the flush, because that is when the deck
    /// has an image to show. Its `bytes` is the length the header promised,
    /// which is the only length the wire carries: the chunk packs are padded to
    /// the report size and say nothing about their payload.
    ImageTransfer {
        /// The 1-based firmware key the image was addressed to.
        firmware_key: u8,
        /// The image length the header declared.
        bytes: usize,
    },
    /// `CRT|CLE` with the `0xFF` selector.
    ClearAll,
    /// `CRT|CLE` addressed to one key.
    ClearKey {
        /// The 1-based firmware key the clear was addressed to.
        firmware_key: u8,
    },
    /// `CRT|CONNECT`, the reference implementation's keep-alive.
    KeepAlive,
    /// `CRT|HAN`, the sleep pack.
    Sleep,
    /// A bare `HAN` at the head of the report, the connect-time greeting.
    Handshake,
    /// `CRT|VER`, the firmware query.
    FirmwareVersion,
    /// `CRT|MOD`, with the mode digit in the pack.
    Mode {
        /// The mode the pack asked for.
        mode: u8,
    },
}

impl Ss550Command {
    /// The registry entry this command is.
    ///
    /// ```
    /// use deck_simulator::Ss550Command;
    /// use ss550_protocol::Command;
    ///
    /// assert_eq!(Ss550Command::ClearAll.command(), Command::ClearAll);
    /// ```
    pub fn command(self) -> Command {
        match self {
            Self::DisplayOn => Command::DisplayOn,
            Self::Brightness { .. } => Command::Brightness,
            Self::ImageTransfer { .. } => Command::ImageTransfer,
            Self::ClearAll => Command::ClearAll,
            Self::ClearKey { .. } => Command::ClearKey,
            Self::KeepAlive => Command::KeepAlive,
            Self::Sleep => Command::Sleep,
            Self::Handshake => Command::Handshake,
            Self::FirmwareVersion => Command::FirmwareVersion,
            Self::Mode { .. } => Command::Mode,
        }
    }

    /// The registry's name for this command.
    ///
    /// ```
    /// use deck_simulator::Ss550Command;
    ///
    /// assert_eq!(Ss550Command::KeepAlive.name(), "keep_alive");
    /// ```
    pub fn name(self) -> &'static str {
        self.command().name()
    }
}

/// What one report turned out to be.
///
/// A chunk pack carries no tag, so a decoder cannot recognise one: only the
/// caller knows whether a chunk is expected. That is why a chunk is not a frame
/// here, and why a transfer is closed by its flush instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Frame {
    /// A complete, self-contained command.
    Command(Ss550Command),
    /// The `CRT|BAT` header of an image, opening a transfer.
    ImageHeader {
        /// The 1-based firmware key the image is addressed to.
        firmware_key: u8,
        /// The image length the header declares.
        bytes: usize,
    },
    /// A `CRT|STP` flush, which commits whatever came before it.
    Flush,
}

/// Why a report was not a command.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum DecodeError {
    /// The report is not one full pack.
    #[error("report is {actual} bytes; a pack is {REPORT_SIZE}")]
    WrongLength {
        /// How long the report actually was.
        actual: usize,
    },
    /// The report carries no `CRT` header and no bare handshake either.
    #[error("report is not a command: no CRT header and no bare handshake")]
    NotACommand,
    /// The `CRT` header was there, the tag after it was not a known one.
    #[error("report carries an unknown tag {tag:02x?}")]
    UnknownTag {
        /// The three bytes where the tag belongs.
        tag: [u8; 3],
    },
    /// A `MOD` pack whose mode byte is not an ASCII digit.
    #[error("mode pack carries {byte:#04x}, which is not an ASCII digit")]
    ModeIsNotADigit {
        /// The byte where the mode digit belongs.
        byte: u8,
    },
}

/// Decodes one report, or says why it is not a command.
pub(crate) fn decode(report: &[u8]) -> Result<Frame, DecodeError> {
    let pack: &[u8; REPORT_SIZE] = report.try_into().map_err(|_| DecodeError::WrongLength {
        actual: report.len(),
    })?;

    if is_flush(pack) {
        return Ok(Frame::Flush);
    }

    if has_prefix(pack, b"CRT") {
        return decode_tagged(pack);
    }

    if has_prefix(pack, b"HAN") {
        return Ok(Frame::Command(Ss550Command::Handshake));
    }

    Err(DecodeError::NotACommand)
}

/// Whether the report is the `CRT|STP` flush.
///
/// The flush is not a command of its own — it is the commit at the end of a
/// transfer or a clear — so it has no registry entry and no tag constant. The
/// bytes are the contract.
pub(crate) fn is_flush(report: &[u8]) -> bool {
    match <&[u8; REPORT_SIZE]>::try_from(report) {
        Ok(pack) => has_prefix(pack, b"CRT") && pack.get(6..9) == Some(b"STP".as_slice()),
        Err(_) => false,
    }
}

/// Decodes the report that carries the `CRT` header.
fn decode_tagged(pack: &[u8; REPORT_SIZE]) -> Result<Frame, DecodeError> {
    // CONNECT is seven bytes, so it does not fit the three-byte tag field that
    // every other command uses.
    if pack.get(6..13) == Some(b"CONNECT".as_slice()) {
        return Ok(Frame::Command(Ss550Command::KeepAlive));
    }

    let frame = match pack.get(6..9) {
        Some(b"DIS") => Frame::Command(Ss550Command::DisplayOn),
        Some(b"LIG") => Frame::Command(Ss550Command::Brightness { value: pack[11] }),
        Some(b"BAT") => Frame::ImageHeader {
            firmware_key: pack[13],
            bytes: usize::from(u16::from_be_bytes([pack[11], pack[12]])),
        },
        Some(b"CLE") if pack[12] == 0xFF => Frame::Command(Ss550Command::ClearAll),
        Some(b"CLE") => Frame::Command(Ss550Command::ClearKey {
            firmware_key: pack[12],
        }),
        Some(b"HAN") => Frame::Command(Ss550Command::Sleep),
        Some(b"VER") => Frame::Command(Ss550Command::FirmwareVersion),
        Some(b"MOD") => Frame::Command(Ss550Command::Mode {
            mode: pack[11]
                .checked_sub(b'0')
                .ok_or(DecodeError::ModeIsNotADigit { byte: pack[11] })?,
        }),
        Some(_) => {
            return Err(DecodeError::UnknownTag {
                tag: [pack[6], pack[7], pack[8]],
            })
        }
        None => return Err(DecodeError::NotACommand),
    };
    Ok(frame)
}

/// Whether the bytes at `[1..4]` are `prefix`.
fn has_prefix(pack: &[u8; REPORT_SIZE], prefix: &[u8]) -> bool {
    pack.get(1..1 + prefix.len()) == Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss550_protocol::{build_connect, build_dis, build_lig, build_mod, build_stp};

    #[test]
    fn a_short_report_is_a_length_problem_not_a_command() {
        assert_eq!(
            decode(&[0_u8; 4]),
            Err(DecodeError::WrongLength { actual: 4 })
        );
    }

    #[test]
    fn the_flush_is_recognised_by_its_bytes() {
        let stp = build_stp();
        assert!(is_flush(stp.as_bytes()));
        assert_eq!(decode(stp.as_bytes()), Ok(Frame::Flush));
    }

    #[test]
    fn a_seven_byte_tag_is_not_read_as_a_three_byte_one() {
        let connect = build_connect();
        assert_eq!(
            decode(connect.as_bytes()),
            Ok(Frame::Command(Ss550Command::KeepAlive))
        );
    }

    #[test]
    fn the_brightness_comes_out_of_the_field_not_out_of_the_tag() {
        let dim = build_lig(7).expect("7 is a valid brightness");
        let bright = build_lig(100).expect("100 is a valid brightness");
        assert_eq!(
            decode(dim.as_bytes()),
            Ok(Frame::Command(Ss550Command::Brightness { value: 7 }))
        );
        assert_eq!(
            decode(bright.as_bytes()),
            Ok(Frame::Command(Ss550Command::Brightness { value: 100 }))
        );
    }

    #[test]
    fn the_mode_byte_is_read_as_a_digit() {
        let mode = build_mod(4).expect("4 is a valid mode");
        assert_eq!(
            decode(mode.as_bytes()),
            Ok(Frame::Command(Ss550Command::Mode { mode: 4 }))
        );
    }

    #[test]
    fn a_wake_pack_is_a_command() {
        let dis = build_dis();
        assert_eq!(
            decode(dis.as_bytes()),
            Ok(Frame::Command(Ss550Command::DisplayOn))
        );
    }

    #[test]
    fn a_pack_with_no_header_is_not_a_command() {
        assert_eq!(
            decode(&[0xAA_u8; REPORT_SIZE]),
            Err(DecodeError::NotACommand)
        );
    }
}

//! The Discord client's local pipe: finding it, and framing on it.
//!
//! Discord's client listens on a named pipe — `discord-ipc-0` through
//! `discord-ipc-9` — and speaks a frame format of its own: an eight-byte header
//! holding the opcode and the payload length, both little-endian `u32`,
//! followed by a JSON payload.
//!
//! Opening the pipe needs no Win32 call of our own. [`std::fs::File`] opens
//! `\\.\pipe\<name>` through `CreateFileW`, which is what lets this crate stay
//! `#![deny(unsafe_code)]` instead of claiming a third exception.
//!
//! ```no_run
//! use ss550_discord::pipe;
//!
//! let (_index, mut file) = pipe::connect().expect("Discord is running");
//! pipe::write_frame(&mut file, pipe::Opcode::Handshake, br#"{"v":1}"#)
//!     .expect("the handshake goes out");
//! ```

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// The prefix of the pipes Discord's client listens on.
const PIPE_PREFIX: &str = r"\\.\pipe\discord-ipc-";

/// How many pipes Discord may be on. Discord documents `0` through `9`, and a
/// client that fails to bind its favourite picks the next one.
const PIPES: u8 = 10;

/// The largest payload this crate will accept, in bytes.
///
/// A frame is a handshake, a command or a voice update: every one of them is
/// under a kilobyte. The bound exists so a corrupt length cannot make this
/// crate allocate a gigabyte, and so a single read of this size always covers a
/// whole message — a message-mode pipe answers `ERROR_MORE_DATA` when the
/// buffer is too small, which would otherwise lose the bytes.
pub const MAX_PAYLOAD: usize = 1 << 20;

/// One read's worth of bytes: the largest frame plus its header.
pub const CHUNK: usize = MAX_PAYLOAD + 8;

/// What a frame says it is.
///
/// The numbers are Discord's, not ours: `0` opens the conversation, `1` carries
/// every command and event, and the rest keep the connection honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    /// The first packet, carrying the protocol version and the application id.
    Handshake,
    /// A command or an event.
    Frame,
    /// Discord asking to be let go, or this side asking the same.
    Close,
    /// A keep-alive.
    Ping,
    /// The answer to a [`Opcode::Ping`].
    Pong,
}

impl Opcode {
    /// The opcode a header's first `u32` means, when it means one of ours.
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Opcode::Handshake),
            1 => Some(Opcode::Frame),
            2 => Some(Opcode::Close),
            3 => Some(Opcode::Ping),
            4 => Some(Opcode::Pong),
            _ => None,
        }
    }

    /// The number this opcode is written as.
    pub fn as_u32(self) -> u32 {
        match self {
            Opcode::Handshake => 0,
            Opcode::Frame => 1,
            Opcode::Close => 2,
            Opcode::Ping => 3,
            Opcode::Pong => 4,
        }
    }
}

/// what came out of [`Frames::next_frame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// A whole frame: what it is, and its payload.
    Frame(Opcode, Vec<u8>),
    /// The bytes so far are the beginning of a frame, not all of one.
    Incomplete,
    /// The header states a payload larger than [`MAX_PAYLOAD`].
    Oversized {
        /// The length the header claimed.
        length: usize,
    },
    /// The header states an opcode this protocol does not define.
    Unknown {
        /// The opcode the header claimed.
        opcode: u32,
    },
}

/// The bytes of a connection, split back into frames.
///
/// A pipe may hand over half a frame, three frames, or exactly one. This keeps
/// the leftover bytes between reads, so a reader never has to know which of
/// those the operating system chose.
///
/// ```
/// use ss550_discord::pipe::{Frames, Next, Opcode};
///
/// let mut frames = Frames::new();
/// // A header and half of a payload: nothing to hand over yet.
/// frames.push(&[1, 0, 0, 0, 4, 0, 0, 0, b'{', b'"']);
/// assert_eq!(frames.next_frame(), Next::Incomplete);
/// frames.push(&[b'v', b'"']);
/// assert_eq!(
///     frames.next_frame(),
///     Next::Frame(Opcode::Frame, b"{\"v\"".to_vec())
/// );
/// ```
#[derive(Debug, Default)]
pub struct Frames {
    bytes: Vec<u8>,
}

impl Frames {
    /// An empty reader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Half a frame, waiting for the rest.
    pub fn push(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    /// The next complete frame, if the bytes hold one.
    ///
    /// The frame's bytes are removed from the buffer, so the next call answers
    /// about the frame after it.
    pub fn next_frame(&mut self) -> Next {
        if self.bytes.len() < 8 {
            return Next::Incomplete;
        }
        let opcode =
            u32::from_le_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]]);
        let length =
            u32::from_le_bytes([self.bytes[4], self.bytes[5], self.bytes[6], self.bytes[7]])
                as usize;
        if length > MAX_PAYLOAD {
            return Next::Oversized { length };
        }
        let Some(opcode) = Opcode::from_u32(opcode) else {
            return Next::Unknown { opcode };
        };
        let end = 8 + length;
        if self.bytes.len() < end {
            return Next::Incomplete;
        }
        let payload = self.bytes[8..end].to_vec();
        self.bytes.drain(..end);
        Next::Frame(opcode, payload)
    }
}

/// Writes one frame: the header, then the payload.
///
/// ```
/// use ss550_discord::pipe::{write_frame, Opcode};
///
/// let mut wire = Vec::new();
/// write_frame(&mut wire, Opcode::Frame, b"hi").expect("a vector takes any frame");
/// assert_eq!(wire, [1, 0, 0, 0, 2, 0, 0, 0, b'h', b'i']);
/// ```
pub fn write_frame(wire: &mut impl Write, opcode: Opcode, payload: &[u8]) -> Result<(), io::Error> {
    if payload.len() > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "a {} byte payload is more than this crate will send ({MAX_PAYLOAD})",
                payload.len()
            ),
        ));
    }
    let mut header = [0u8; 8];
    header[..4].copy_from_slice(&opcode.as_u32().to_le_bytes());
    header[4..].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    wire.write_all(&header)?;
    wire.write_all(payload)?;
    wire.flush()
}

/// A pipe that could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// No pipe answered: the client is not running, or not logged in.
    #[error("no Discord client is listening on {tried} pipes")]
    NoClient {
        /// How many pipe names were tried.
        tried: u8,
    },
    /// A pipe was found but would not open.
    #[error("the Discord pipe could not be opened: {source}")]
    Io {
        /// The operating system's reason.
        #[source]
        source: io::Error,
    },
}

/// The path of one of Discord's pipes.
pub fn pipe_path(index: u8) -> PathBuf {
    PathBuf::from(format!("{PIPE_PREFIX}{index}"))
}

/// Opens the first pipe Discord answers on.
///
/// Returns which pipe it was — a second client of the same machine would be on
/// another one — and the open handle.
pub fn connect() -> Result<(u8, File), ConnectError> {
    let mut last = None;
    for index in 0..PIPES {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe_path(index))
        {
            Ok(file) => return Ok((index, file)),
            // A pipe nothing listens on is not an error on its own: the next
            // name is the one that might answer.
            Err(error) => last = Some(error),
        }
    }
    match last {
        Some(source) if source.kind() != io::ErrorKind::NotFound => {
            Err(ConnectError::Io { source })
        }
        _ => Err(ConnectError::NoClient { tried: PIPES }),
    }
}

/// Reads one chunk of bytes, blocking until there are some.
///
/// The buffer is [`CHUNK`] bytes, which is a whole frame plus its header: on a
/// message-mode pipe one call is one message, and on a byte-mode pipe the bytes
/// accumulate in [`Frames`] until a frame is whole.
pub fn read_chunk(wire: &mut impl Read, buffer: &mut [u8]) -> Result<usize, io::Error> {
    wire.read(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
        let mut wire = Vec::new();
        write_frame(&mut wire, opcode, payload).expect("a vector takes any frame");
        wire
    }

    #[test]
    fn a_frame_survives_the_round_trip() {
        let wire = framed(Opcode::Frame, br#"{"cmd":"PING"}"#);
        let mut frames = Frames::new();
        frames.push(&wire);
        assert_eq!(
            frames.next_frame(),
            Next::Frame(Opcode::Frame, br#"{"cmd":"PING"}"#.to_vec())
        );
        assert_eq!(frames.next_frame(), Next::Incomplete);
    }

    #[test]
    fn the_header_says_the_opcode_and_the_length_little_endian() {
        let wire = framed(Opcode::Ping, b"ab");
        assert_eq!(wire, [3, 0, 0, 0, 2, 0, 0, 0, b'a', b'b']);
    }

    #[test]
    fn several_frames_in_one_read_come_out_one_at_a_time() {
        let mut wire = framed(Opcode::Frame, b"one");
        wire.extend(framed(Opcode::Frame, b"two"));
        let mut frames = Frames::new();
        frames.push(&wire);

        assert_eq!(
            frames.next_frame(),
            Next::Frame(Opcode::Frame, b"one".to_vec()),
            "the first frame is the first frame"
        );
        assert_eq!(
            frames.next_frame(),
            Next::Frame(Opcode::Frame, b"two".to_vec()),
            "the second follows it"
        );
        assert_eq!(frames.next_frame(), Next::Incomplete);
    }

    #[test]
    fn a_frame_split_across_two_reads_is_still_one_frame() {
        let wire = framed(Opcode::Frame, b"split");
        let mut frames = Frames::new();
        for byte in &wire[..5] {
            frames.push(std::slice::from_ref(byte));
        }
        assert_eq!(
            frames.next_frame(),
            Next::Incomplete,
            "five bytes is not a header"
        );

        frames.push(&wire[5..]);
        assert_eq!(
            frames.next_frame(),
            Next::Frame(Opcode::Frame, b"split".to_vec())
        );
    }

    #[test]
    fn an_empty_payload_is_a_whole_frame() {
        let wire = framed(Opcode::Close, b"");
        let mut frames = Frames::new();
        frames.push(&wire);
        assert_eq!(frames.next_frame(), Next::Frame(Opcode::Close, Vec::new()));
    }

    #[test]
    fn a_length_past_the_cap_is_refused_before_it_allocates() {
        let mut frames = Frames::new();
        let too_long = (MAX_PAYLOAD as u32 + 1).to_le_bytes();
        frames.push(&[
            1,
            0,
            0,
            0,
            too_long[0],
            too_long[1],
            too_long[2],
            too_long[3],
        ]);
        assert_eq!(
            frames.next_frame(),
            Next::Oversized {
                length: MAX_PAYLOAD + 1
            }
        );
    }

    #[test]
    fn an_opcode_this_protocol_does_not_define_says_so() {
        let mut frames = Frames::new();
        frames.push(&[9, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(frames.next_frame(), Next::Unknown { opcode: 9 });
    }

    #[test]
    fn a_payload_past_the_cap_is_never_sent() {
        let mut wire = Vec::new();
        let error = write_frame(&mut wire, Opcode::Frame, &vec![0u8; MAX_PAYLOAD + 1])
            .expect_err("too large to send");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            wire.is_empty(),
            "nothing is written when the frame is refused"
        );
    }

    #[test]
    fn the_pipe_names_are_the_ones_discord_documents() {
        assert_eq!(pipe_path(0).to_string_lossy(), r"\\.\pipe\discord-ipc-0");
        assert_eq!(pipe_path(9).to_string_lossy(), r"\\.\pipe\discord-ipc-9");
    }

    #[test]
    fn a_chunk_read_hands_over_what_the_wire_gave() {
        // The read half is a trait, so the framing is exercised without a pipe:
        // what the operating system delivers is the only part a live run adds.
        let wire = framed(Opcode::Frame, b"x");
        let mut cursor = Cursor::new(wire);
        let mut buffer = vec![0u8; CHUNK];
        let read = read_chunk(&mut cursor, &mut buffer).expect("a cursor reads");
        assert_eq!(read, 9);

        let mut frames = Frames::new();
        frames.push(&buffer[..read]);
        assert_eq!(
            frames.next_frame(),
            Next::Frame(Opcode::Frame, b"x".to_vec())
        );
    }
}

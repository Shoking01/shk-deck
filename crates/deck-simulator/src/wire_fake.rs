//! The SS550 wire, emulated at the transport seam.
//!
//! This is the second implementation at the *transport* seam, and its job is
//! narrower than the deck's: move bytes, decode enough of them to know what was
//! asked for, and be able to fail. It has no opinion about key numbering, image
//! formats or capabilities — a deck sits on top of it and owns all of that.
//!
//! Nothing is written anywhere. The fake keeps a transcript of the commands it
//! understood, so a test can assert what a deck asked the wire to do without a
//! device and without `hidapi`.
//!
//! **It is not a safety gate.** The verified-command registry is enforced by the
//! driver before the bytes leave the host. This is a double for tests, and it
//! accepts anything shaped like a pack.

use std::collections::VecDeque;
use std::time::Duration;

use deck_api::{Transport, TransportError};
use ss550_protocol::{CHUNK_SIZE, REPORT_SIZE};

use crate::wire::{self, DecodeError, Frame, Ss550Command};

/// An image whose header has arrived and whose flush has not.
#[derive(Debug)]
struct OpenTransfer {
    /// The 1-based firmware key the header addressed.
    firmware_key: u8,
    /// The image length the header declared.
    expected: usize,
    /// How many chunk packs have arrived since the header.
    chunks: usize,
}

/// A device that speaks the SS550 pack format and nothing else.
///
/// ```
/// use deck_api::Transport;
/// use deck_simulator::Ss550WireFake;
/// use ss550_protocol::build_lig;
///
/// let mut fake = Ss550WireFake::new();
/// let pack = build_lig(40).expect("40 is a valid brightness");
///
/// fake.write_report(pack.as_bytes()).expect("the fake takes a pack");
/// assert_eq!(fake.command_names(), ["brightness"]);
/// ```
#[derive(Debug)]
pub struct Ss550WireFake {
    /// Every command the fake understood, in the order it arrived.
    transcript: Vec<Ss550Command>,
    /// The image awaiting its flush, if any.
    open: Option<OpenTransfer>,
    /// A failure to report on the next write instead of accepting it.
    fail_next_write: Option<TransportError>,
    /// A failure to report on the next read instead of answering it.
    fail_next_read: Option<TransportError>,
    /// Input reports a test has queued for the next read.
    input: VecDeque<Vec<u8>>,
}

impl Ss550WireFake {
    /// A fake with nothing recorded, nothing open and nothing queued.
    ///
    /// ```
    /// use deck_simulator::Ss550WireFake;
    ///
    /// assert!(Ss550WireFake::new().transcript().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            transcript: Vec::new(),
            open: None,
            fail_next_write: None,
            fail_next_read: None,
            input: VecDeque::new(),
        }
    }

    /// Every command the fake understood, in arrival order.
    ///
    /// An image is one entry, recorded at its flush: the header, the chunks and
    /// the commit are one request from the deck's point of view.
    ///
    /// ```
    /// use deck_api::Transport;
    /// use deck_simulator::{Ss550Command, Ss550WireFake};
    /// use ss550_protocol::build_lig;
    ///
    /// let mut fake = Ss550WireFake::new();
    /// let pack = build_lig(30).expect("30 is a valid brightness");
    /// fake.write_report(pack.as_bytes()).expect("the fake takes a pack");
    ///
    /// assert_eq!(fake.transcript(), [Ss550Command::Brightness { value: 30 }]);
    /// ```
    pub fn transcript(&self) -> &[Ss550Command] {
        &self.transcript
    }

    /// The registry names of the commands in the transcript, in order.
    ///
    /// ```
    /// use deck_api::Transport;
    /// use deck_simulator::Ss550WireFake;
    /// use ss550_protocol::build_dis;
    ///
    /// let mut fake = Ss550WireFake::new();
    /// let pack = build_dis();
    /// fake.write_report(pack.as_bytes()).expect("the fake takes a pack");
    ///
    /// assert_eq!(fake.command_names(), ["display_on"]);
    /// ```
    pub fn command_names(&self) -> Vec<&'static str> {
        self.transcript.iter().map(|entry| entry.name()).collect()
    }

    /// Makes the next write return `error` instead of accepting the pack.
    ///
    /// One instruction covers one write: the write after it is a normal write,
    /// because a transport that stayed broken would make every later assertion
    /// in the test a guess about the previous one.
    ///
    /// ```
    /// use deck_api::{Transport, TransportError};
    /// use deck_simulator::Ss550WireFake;
    /// use ss550_protocol::build_dis;
    ///
    /// let mut fake = Ss550WireFake::new();
    /// fake.fail_next_write(TransportError::Stalled("the cable is out".to_owned()));
    ///
    /// let pack = build_dis();
    /// assert!(fake.write_report(pack.as_bytes()).is_err());
    /// assert!(fake.transcript().is_empty());
    /// ```
    pub fn fail_next_write(&mut self, error: TransportError) {
        self.fail_next_write = Some(error);
    }

    /// Makes the next read return `error` instead of answering.
    ///
    /// [`TransportError::Stalled`] is the honest shape for a device that
    /// stopped answering; nothing in the seam stops a test from using either
    /// variant.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use deck_api::{Transport, TransportError};
    /// use deck_simulator::Ss550WireFake;
    ///
    /// let mut fake = Ss550WireFake::new();
    /// fake.fail_next_read(TransportError::Io("the endpoint halted".to_owned()));
    ///
    /// let mut buffer = [0_u8; 512];
    /// assert!(fake.read_input(&mut buffer, Duration::ZERO).is_err());
    /// ```
    pub fn fail_next_read(&mut self, error: TransportError) {
        self.fail_next_read = Some(error);
    }

    /// Queues an input report for the next read.
    ///
    /// The bytes are handed up untouched: decoding a button report is the
    /// deck's job, not the transport's.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use deck_api::Transport;
    /// use deck_simulator::Ss550WireFake;
    ///
    /// let mut fake = Ss550WireFake::new();
    /// fake.queue_input_report(&[0x41, 0x43, 0x4B]);
    ///
    /// let mut buffer = [0_u8; 512];
    /// assert_eq!(fake.read_input(&mut buffer, Duration::ZERO).expect("a read"), 3);
    /// ```
    pub fn queue_input_report(&mut self, report: &[u8]) {
        self.input.push_back(report.to_vec());
    }

    /// Records the committed transfer, after checking the chunk count.
    ///
    /// The chunk count is what can be checked: the packs are padded to the
    /// report size, so the bytes on the wire say nothing about how many of them
    /// carried payload. The header is the only statement of length, and the
    /// number of packs has to match it exactly.
    fn commit(&mut self, open: OpenTransfer) -> Result<(), TransportError> {
        let expected_chunks = open.expected.div_ceil(CHUNK_SIZE);
        if open.chunks != expected_chunks {
            return Err(TransportError::Io(format!(
                "the image header promised {} bytes ({expected_chunks} chunk packs) and {} chunk packs arrived",
                open.expected, open.chunks
            )));
        }
        self.transcript.push(Ss550Command::ImageTransfer {
            firmware_key: open.firmware_key,
            bytes: open.expected,
        });
        Ok(())
    }
}

impl Default for Ss550WireFake {
    /// The same as [`Ss550WireFake::new`].
    ///
    /// ```
    /// use deck_simulator::Ss550WireFake;
    ///
    /// assert!(Ss550WireFake::default().transcript().is_empty());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for Ss550WireFake {
    /// Decodes one report and records what it was.
    ///
    /// A report that is not one full pack, or that carries no command this
    /// device speaks, is refused with [`TransportError::Io`] rather than
    /// swallowed: a double that accepts anything cannot tell a deck writing the
    /// right bytes from one writing garbage.
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError> {
        if let Some(error) = self.fail_next_write.take() {
            return Err(error);
        }

        if bytes.len() != REPORT_SIZE {
            return Err(TransportError::Io(format!(
                "report is {} bytes; a pack is {REPORT_SIZE}",
                bytes.len()
            )));
        }

        // While an image is open, the packs are its payload: they carry no tag,
        // so only the flush can end the sequence.
        if let Some(mut open) = self.open.take() {
            if wire::is_flush(bytes) {
                self.commit(open)?;
            } else {
                open.chunks += 1;
                self.open = Some(open);
            }
            return Ok(REPORT_SIZE);
        }

        match wire::decode(bytes).map_err(decode_failure)? {
            Frame::Command(command) => self.transcript.push(command),
            Frame::ImageHeader {
                firmware_key,
                bytes: length,
            } => {
                self.open = Some(OpenTransfer {
                    firmware_key,
                    expected: length,
                    chunks: 0,
                });
            }
            // A flush with nothing open commits nothing: it is the second half
            // of a clear, which was recorded when the clear arrived.
            Frame::Flush => {}
        }

        Ok(REPORT_SIZE)
    }

    /// Answers with a queued report, or with nothing.
    ///
    /// The timeout is ignored on purpose: the fake has nothing to wait for, and
    /// a transport double that sleeps is a test suite that sleeps. Zero bytes
    /// means nothing was waiting, which is the seam's normal answer. A queued
    /// report longer than `buf` is truncated and the remainder dropped, which is
    /// what a real fixed-size input buffer does.
    fn read_input(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize, TransportError> {
        if let Some(error) = self.fail_next_read.take() {
            return Err(error);
        }

        let Some(report) = self.input.pop_front() else {
            return Ok(0);
        };

        let copied = report.len().min(buf.len());
        if let (Some(destination), Some(source)) = (buf.get_mut(..copied), report.get(..copied)) {
            destination.copy_from_slice(source);
        }
        Ok(copied)
    }

    /// Answers with nothing: this device sends no control reports.
    fn read_control(&mut self, _buf: &mut [u8]) -> Result<usize, TransportError> {
        Ok(0)
    }
}

/// Turns a decode failure into the seam's transport error.
fn decode_failure(error: DecodeError) -> TransportError {
    TransportError::Io(error.to_string())
}

//! # ss550-protocol
//!
//! Pure HID protocol codec for the Redragon StreamCraft SS550 v3
//! (VID 0x0200 / PID 0x3000).
//!
//! This crate has **no I/O, no USB, no `std::time`** — only encoding and
//! decoding of the wire protocol. Drivers own the transport; this crate
//! owns the bytes.
//!
//! ## Wire summary
//!
//! - Output report length: **1025 B** (report ID `0x00` + 1024 payload).
//! - Input report length: **512 B and no report ID byte** — the directions are
//!   asymmetric. See [`parse_input_report`].
//! - Every `CRT` command: `[0]=0x00`, `[1..4]="CRT"`, `[4]=0`, `[5]=0`,
//!   `[6..]=tag`. Fields follow the tag; every pack is zero-padded to 1025.
//! - Single-image path (verified live): `DIS`, `LIG 0`, then per key
//!   `BAT` + JPEG chunks, one `STP`, then `LIG 100`.
//! - Button edges (verified live): `CRT|ACK` header with `"ACK"` at `[0..3]`,
//!   `"OK"` at `[5..7]`, key at `[9]`, state at `[10]`.
//!
//! ## The verified list
//!
//! The `verified` module is the machine-readable safe-mode list: for every
//! command the protocol can encode, [`Command`] says whether a **test deck**
//! has proven it and where the evidence lives. The driver refuses to send
//! anything that is not on it. The human-readable twin is
//! `docs/protocol/verified.md`, and a test fails when the two disagree.
//!
//! ```rust
//! use ss550_protocol::{Command, COMMANDS};
//!
//! assert_eq!(COMMANDS.len(), 10);
//! assert!(Command::DisplayOn.is_verified());
//! assert!(!Command::FirmwareVersion.is_verified());
//! ```
//!
//! ## Example
//!
//! ```rust
//! use ss550_protocol::{build_bat, build_lig, build_stp};
//!
//! // Brightness 30 → wire[11] = 30.
//! let lig = build_lig(30).expect("30 is valid");
//! assert_eq!(lig.as_bytes()[11], 30);
//!
//! // BAT header for a 4660-byte image on firmware key 7.
//! let bat = build_bat(4660, 7).expect("valid header");
//! assert_eq!(bat.as_bytes()[11], 0x12);
//! assert_eq!(bat.as_bytes()[12], 0x34);
//! assert_eq!(bat.as_bytes()[13], 7);
//!
//! let _stp = build_stp();
//! ```
//!
//! Byte-exact reference: `4ndv/mirajazz` `src/device.rs`.

#![deny(unsafe_code)]

mod error;
mod input;
mod pack;
mod verified;

pub const VID: u16 = 0x0200;
pub const PID: u16 = 0x3000;

pub use error::ProtocolError;
pub use input::{
    is_key_edge, parse_input_report, read_index_to_firmware_key, ButtonState, InputEvent,
    INPUT_REPORT_SIZE,
};
pub use pack::{
    build_bat, build_cle_all, build_cle_key, build_connect, build_dis, build_han_bare,
    build_image_chunk, build_image_chunks, build_lig, build_mod, build_sleep, build_stp, build_ver,
    Pack, CHUNK_SIZE, MAX_IMAGE_SIZE, REPORT_SIZE,
};
pub use verified::{Command, CommandInfo, Verification, COMMANDS};

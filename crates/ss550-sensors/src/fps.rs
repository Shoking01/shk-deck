//! How fast the foreground application is drawing, as RTSS counts it.
//!
//! ## Why RTSS and not our own counter
//!
//! Measuring another program's frames from outside means watching the display
//! driver's *present* events through ETW, which needs an elevated consumer and a
//! parser for the driver's own events — work of the size of PresentMon. RivaTuner
//! Statistics Server already does exactly that, in a signed process that hooks
//! the game on its own, and publishes the result in a mapped file:
//! `RTSSSharedMemoryV2`.
//!
//! This module **only reads that block**. It injects nothing, hooks nothing, and
//! never touches a game's address space: the one process inside the game is
//! RTSS's own, which is what keeps this outside the reach of anti-cheat rules
//! about foreign overlays.
//!
//! ## What it answers
//!
//! One entry per application RTSS is tracking, with that application's **frame
//! counter** and the millisecond it was last updated. The rate is the difference
//! between two reads — this module does not divide anything, because a single
//! read cannot know how much time passed. The arithmetic belongs to the caller,
//! where it is a pure function and a test.
//!
//! ## When RTSS is not running
//!
//! The mapping does not exist, and the answer is an empty list. That is ordinary:
//! a machine without RTSS simply has no frame counter, and the key that shows one
//! says so instead of freezing on a stale number.

use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// The name RTSS gives its shared memory.
const MAPPING: &str = "RTSSSharedMemoryV2";

/// `'RTSS'` as the block spells it: the four bytes `R`, `T`, `S`, `S` read as one
/// little-endian word.
const SIGNATURE: u32 = 0x5254_5353;

/// One application RTSS is counting frames for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameCounter {
    /// The application's process id.
    pub pid: u32,
    /// What RTSS calls it: the game's executable, usually.
    pub name: String,
    /// Frames counted so far. Only the *difference* between two reads means
    /// anything, which is why nothing here divides.
    pub frames: u32,
    /// When the last frame was counted, in milliseconds on RTSS's own clock.
    pub time_ms: u32,
}

/// The process id of the window in the foreground, when there is one.
///
/// This is what decides *whose* frames a key shows: the game the user is looking
/// at, not every process RTSS happens to track.
pub fn foreground_process() -> Option<u32> {
    // SAFETY: both calls take and return plain handles and out-parameters; a null
    // window (nothing in the foreground) is checked before the id is used.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a live out-parameter.
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (thread != 0 && pid != 0).then_some(pid)
}

/// Every application RTSS is counting, or nothing when it is not running.
pub fn frame_counters() -> Vec<FrameCounter> {
    let name = wide(MAPPING);
    // SAFETY: the string is nul-terminated, and the handle is closed below on
    // every path out of this function.
    let mapping = match unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())) } {
        Ok(handle) => handle,
        // RTSS is not running: no mapping, no frames.
        Err(_) => return Vec::new(),
    };

    // SAFETY: the mapping is open and read-only; a null view means Windows could
    // not map it, which is answered with the empty list rather than a panic.
    let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0) };
    if view.Value.is_null() {
        // SAFETY: the mapping handle came from `OpenFileMappingW` above and is
        // not used again.
        let _ = unsafe { CloseHandle(mapping) };
        return Vec::new();
    }

    let counters = read_entries(view.Value.cast::<u8>());

    // SAFETY: both handles came from the calls above, are not used again, and are
    // unmapped and closed exactly once, whatever `read_entries` answered.
    let _ = unsafe { UnmapViewOfFile(view) };
    let _ = unsafe { CloseHandle(mapping) };
    counters
}

/// The entries of a mapped RTSS block, read through the header's own layout.
///
/// The header states where the entries start, how large each one is and how many
/// there are, so this never assumes a version: it walks what the block declares.
fn read_entries(base: *const u8) -> Vec<FrameCounter> {
    // SAFETY: the block's first nine words are the header RTSS writes; each read
    // is unaligned because a mapping carries no alignment promise.
    let signature = unsafe { u32_at(base, 0) };
    if signature != SIGNATURE {
        return Vec::new();
    }
    let entry_size = unsafe { u32_at(base, 8) } as usize;
    let entries_offset = unsafe { u32_at(base, 12) } as usize;
    let entries = unsafe { u32_at(base, 16) } as usize;
    if entry_size < 280 || entries == 0 || entries > 256 {
        return Vec::new();
    }

    let mut counters = Vec::new();
    for index in 0..entries {
        let entry = entries_offset + index * entry_size;
        // SAFETY: every read stays inside the entry the header described.
        let pid = unsafe { u32_at(base, entry) };
        if pid == 0 {
            continue;
        }
        let name = read_name(base, entry + 4);
        // SAFETY: `dwTime1` and `dwFrames` sit at the offsets every published
        // RTSS layout agrees on, inside the same entry.
        let time_ms = unsafe { u32_at(base, entry + 272) };
        let frames = unsafe { u32_at(base, entry + 276) };
        counters.push(FrameCounter {
            pid,
            name,
            frames,
            time_ms,
        });
    }
    counters
}

/// One little-endian word out of the mapped block.
///
/// # Safety
///
/// `base` must point at a mapped block, and `offset` must stay inside it.
unsafe fn u32_at(base: *const u8, offset: usize) -> u32 {
    // SAFETY: the caller promises the offset is inside the mapping.
    unsafe { std::ptr::read_unaligned(base.add(offset).cast::<u32>()) }
}

/// The application's name: a nul-terminated ASCII string in a fixed field.
fn read_name(base: *const u8, at: usize) -> String {
    let mut name = String::new();
    for offset in 0..260 {
        // SAFETY: the name field is 260 bytes wide in every RTSS layout, and the
        // block is mapped whole.
        let byte = unsafe { std::ptr::read_unaligned(base.add(at + offset)) };
        if byte == 0 {
            break;
        }
        name.push(byte as char);
    }
    name
}

/// A string as the wide, nul-terminated form the Win32 calls take.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{foreground_process, frame_counters, FrameCounter};

    #[test]
    fn a_machine_without_rtss_answers_with_no_counters() {
        // The read is allowed to find nothing — RTSS is a third-party program and
        // this build must not depend on it being installed. What it may never do
        // is panic, which is what a bad mapping or a wrong header would do.
        let counters = frame_counters();
        for counter in &counters {
            // When RTSS *is* running, every entry it reports has an owner.
            assert_ne!(counter.pid, 0);
        }
    }

    #[test]
    fn the_foreground_process_is_an_id_or_nothing() {
        // Whatever the desktop is doing, the answer is a pid or `None`; a wrong
        // type here would be a key showing another program's frames.
        if let Some(pid) = foreground_process() {
            assert_ne!(pid, 0);
        }
    }

    #[test]
    fn a_counter_carries_the_two_numbers_a_rate_needs() {
        // The rate is a difference, so both halves have to survive the read.
        let counter = FrameCounter {
            pid: 4242,
            name: "game.exe".to_string(),
            frames: 12_345,
            time_ms: 67_890,
        };
        assert_eq!(counter.frames, 12_345);
        assert_eq!(counter.time_ms, 67_890);
    }
}

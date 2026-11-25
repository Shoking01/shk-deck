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

#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::CloseHandle;
#[cfg(windows)]
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// The name RTSS gives its shared memory.
#[cfg(windows)]
const MAPPING: &str = "RTSSSharedMemoryV2";

/// `'RTSS'` as the block spells it: the four bytes `R`, `T`, `S`, `S` read as one
/// little-endian word.
#[cfg(windows)]
const SIGNATURE: u32 = 0x5254_5353;

/// One application RTSS is counting frames for.
///
/// The three numbers that matter are a **period**, not a running total: RTSS
/// rotates `time0`, `time1` and `frames` once per second, and its own header says
/// how to read them:
///
/// ```text
/// 1000.0 * dwFrames / (dwTime1 - dwTime0)
/// or
/// 1000000.0 / dwFrameTime
/// ```
///
/// Subtracting two of *our* samples would compare two different periods — which is
/// how a key came to show one to three frames per second for a game running at
/// sixty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameCounter {
    /// The application's process id.
    pub pid: u32,
    /// What RTSS calls it: the game's executable, usually.
    pub name: String,
    /// Frames rendered during the measurement period.
    pub frames: u32,
    /// When the measurement period started, in milliseconds. Zero until RTSS has
    /// seen two moments, and its header is explicit that a zero cannot be used.
    pub time0: u32,
    /// When the measurement period ended, in milliseconds.
    pub time1: u32,
    /// The last frame's time, in microseconds.
    pub frame_time_us: u32,
}

impl FrameCounter {
    /// The rate RTSS is reporting, by the header's own formula.
    ///
    /// `None` when the period is unusable: RTSS has not measured two moments yet
    /// (`time0` zero, or `time1` not after it), which is what the first second of
    /// a freshly hooked game looks like.
    pub fn fps(&self) -> Option<f32> {
        if self.time0 == 0 || self.time1 <= self.time0 {
            return None;
        }
        let elapsed = self.time1.checked_sub(self.time0)?;
        let frames = f64::from(self.frames) * 1000.0 / f64::from(elapsed);
        frames.is_finite().then_some(frames as f32)
    }
}

/// The process id of the window in the foreground, when there is one.
///
/// This is what decides *whose* frames a key shows: the game the user is looking
/// at, not every process RTSS happens to track.
#[cfg(windows)]
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

/// The process id of the window in the foreground, when there is one.
///
/// RTSS is a Windows program and this build reads nothing else, so on any other
/// platform there is no frame counter to attribute and the answer is `None`
/// rather than an owner invented from a window manager that has no RTSS.
#[cfg(not(windows))]
pub fn foreground_process() -> Option<u32> {
    None
}

/// Every application RTSS is counting, or nothing when it is not running.
#[cfg(windows)]
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

/// Every application RTSS is counting, or nothing when it is not running.
///
/// RTSS is a Windows program: elsewhere the mapping does not exist, which is the
/// same answer a Windows machine without RTSS gets.
#[cfg(not(windows))]
pub fn frame_counters() -> Vec<FrameCounter> {
    Vec::new()
}

/// The entries of a mapped RTSS block, read through the header's own layout.
///
/// The header states where the entries start, how large each one is and how many
/// there are, so this never assumes a version: it walks what the block declares.
#[cfg(windows)]
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
        // SAFETY: the fields sit at the offsets RTSS's own header states, inside
        // the entry the header described: `dwTime0`, `dwTime1`, `dwFrames`,
        // `dwFrameTime`.
        let time0 = unsafe { u32_at(base, entry + 268) };
        let time1 = unsafe { u32_at(base, entry + 272) };
        let frames = unsafe { u32_at(base, entry + 276) };
        let frame_time_us = unsafe { u32_at(base, entry + 280) };
        counters.push(FrameCounter {
            pid,
            name,
            frames,
            time0,
            time1,
            frame_time_us,
        });
    }
    counters
}

/// One little-endian word out of the mapped block.
///
/// # Safety
///
/// `base` must point at a mapped block, and `offset` must stay inside it.
#[cfg(windows)]
unsafe fn u32_at(base: *const u8, offset: usize) -> u32 {
    // SAFETY: the caller promises the offset is inside the mapping.
    unsafe { std::ptr::read_unaligned(base.add(offset).cast::<u32>()) }
}

/// The application's name: a nul-terminated ASCII string in a fixed field.
#[cfg(windows)]
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
#[cfg(windows)]
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
        // A period, as the header states it: sixty frames in one second is sixty
        // per second, with the same three numbers RTSS publishes.
        let counter = FrameCounter {
            pid: 4242,
            name: "game.exe".to_string(),
            frames: 60,
            time0: 50_000,
            time1: 51_000,
            frame_time_us: 16_667,
        };
        assert_eq!(counter.fps(), Some(60.0));
    }

    #[test]
    fn a_period_that_has_not_started_is_no_measurement() {
        // The header is explicit: `dwTime0` has to be non-zero. A game hooked a
        // moment ago has not measured two moments yet.
        let fresh = FrameCounter {
            pid: 1,
            name: "game.exe".to_string(),
            frames: 0,
            time0: 0,
            time1: 0,
            frame_time_us: 0,
        };
        assert_eq!(fresh.fps(), None);

        let empty_period = FrameCounter {
            time1: 50_000,
            ..fresh
        };
        assert_eq!(
            empty_period.fps(),
            None,
            "a period of no length divides by nothing"
        );
    }
}

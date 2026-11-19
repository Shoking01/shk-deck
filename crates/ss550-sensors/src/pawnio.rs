//! PawnIO's driver: a Ryzen's own thermal block, with no monitoring app running.
//!
//! A processor's temperature lives in registers only a kernel driver may read, so
//! there is no user-mode API for it — on any machine, from any vendor. PawnIO is
//! that driver: signed, GPL-2.0, installed once, and **already present** on a
//! machine that has run LibreHardwareMonitor, because that is how
//! LibreHardwareMonitor reads a processor.
//!
//! What this module needs from it is small. `PawnIOLib.dll` (installed beside the
//! driver) exposes four functions — open, load, execute, close — and a module
//! blob (`resources/pawnio/AMDFamily17.bin`, LGPL-2.1, see its README) provides
//! one primitive: `ioctl_read_smn`, which reads a single System Management
//! Network register.
//!
//! The register that answers is `THM_TCON_CUR_TMP` at `0x00059800`, the thermal
//! control block of every Family 17h processor (Zen, Zen+, Zen 2, Zen 3):
//!
//! ```text
//! raw      = ioctl_read_smn(0x00059800)
//! celsius  = ((raw >> 21) & 0x7FF) * 0.125
//! if (raw & 0x80000) != 0 || (raw & 0x30000) == 0x30000 { celsius -= 49.0 }
//! ```
//!
//! The subtraction is the 49 °C offset every Zen part applies to Tctl above a
//! certain die temperature, and the two flag bits are the two ways the silicon
//! says it has applied it. Both are read here the way Linux's `k10temp` and
//! LibreHardwareMonitor read them.
//!
//! Read-only: the module's write primitives are never called and never declared.

use std::os::raw::c_void;
use std::path::PathBuf;

use libloading::Library;

use crate::usable_celsius;

/// The module blob, embedded: a signature-checked piece of bytecode, not code.
const MODULE: &[u8] = include_bytes!("../resources/pawnio/AMDFamily17.bin");

/// The thermal control block of a Family 17h processor.
const THM_TCON_CUR_TMP: u64 = 0x0005_9800;

/// The bit that says the 49 °C offset has been applied.
const TEMP_RANGE_SEL: u32 = 0x0008_0000;

/// The other way the silicon says the same thing, on newer parts.
const TEMP_TJ_SEL: u32 = 0x0003_0000;

/// The offset those bits mean, in degrees.
const OFFSET_CELSIUS: f32 = 49.0;

/// A processor sensor, opened once.
pub struct Cpu {
    /// The library, kept alive for as long as the handle is.
    _library: Library,
    close: unsafe extern "system" fn(*mut c_void) -> i32,
    execute: Execute,
    handle: *mut c_void,
}

/// `pawnio_execute`, as the header declares it.
type Execute = unsafe extern "system" fn(
    *mut c_void,
    *const std::os::raw::c_char,
    *const u64,
    usize,
    *mut u64,
    usize,
    *mut usize,
) -> i32;

// The handle is the driver's, and the sampling thread is the only one that uses
// it; nothing about it is shareable, only movable.
unsafe impl Send for Cpu {}

/// `S_OK`, the only result that means the answer can be used.
const S_OK: i32 = 0;

impl Cpu {
    /// Opens the driver and loads the AMD module.
    ///
    /// `None` when PawnIO is not installed, when the service is not running, or
    /// when the processor is not a Family 17h part: all three are absences, not
    /// faults, and each costs a dash on a key.
    pub fn open() -> Option<Self> {
        let path = library_path()?;
        // SAFETY: loading a library runs its initialisers; this one is PawnIO's
        // own, shipped with a signed driver.
        let library = unsafe { Library::new(&path) }.ok()?;

        // SAFETY: the names and signatures are the ones `PawnIOLib.h` states.
        let open: unsafe extern "system" fn(*mut *mut c_void) -> i32 =
            unsafe { *library.get(b"pawnio_open\0").ok()? };
        let load: unsafe extern "system" fn(*mut c_void, *const u8, usize) -> i32 =
            unsafe { *library.get(b"pawnio_load\0").ok()? };
        let execute: Execute = unsafe { *library.get(b"pawnio_execute\0").ok()? };
        let close: unsafe extern "system" fn(*mut c_void) -> i32 =
            unsafe { *library.get(b"pawnio_close\0").ok()? };

        let mut handle: *mut c_void = std::ptr::null_mut();
        // SAFETY: `handle` is a valid out pointer and the library is loaded.
        if unsafe { open(&mut handle) } != S_OK || handle.is_null() {
            return None;
        }
        // SAFETY: the blob is static bytes of the size given, and the handle came
        // from `pawnio_open`.
        let loaded = unsafe { load(handle, MODULE.as_ptr(), MODULE.len()) };
        if loaded != S_OK {
            // SAFETY: the handle is ours and still open.
            unsafe { close(handle) };
            return None;
        }

        Some(Self {
            _library: library,
            close,
            execute,
            handle,
        })
    }

    /// The processor's package temperature (Tctl), in whole degrees.
    pub fn temperature(&self) -> Option<f32> {
        let mut out = [0_u64; 1];
        let mut written = 0_usize;
        let name = b"ioctl_read_smn\0";
        // SAFETY: the handle is open, the name is the module's own entry point,
        // the input is one register address and the output buffer is one entry.
        let status = unsafe {
            (self.execute)(
                self.handle,
                name.as_ptr() as *const std::os::raw::c_char,
                &THM_TCON_CUR_TMP,
                1,
                out.as_mut_ptr(),
                1,
                &mut written,
            )
        };
        if status != S_OK || written != 1 {
            return None;
        }
        usable_celsius_f32(celsius_from_smn(out[0] as u32)?)
    }
}

impl Drop for Cpu {
    fn drop(&mut self) {
        // SAFETY: the handle came from `pawnio_open` and is closed exactly once.
        unsafe { (self.close)(self.handle) };
    }
}

/// Where PawnIO's library lives.
///
/// It is not on the search path and not in `System32`: the installer puts it
/// beside the driver, and never in a place a bare name would find.
fn library_path() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles")?;
    let path = PathBuf::from(program_files)
        .join("PawnIO")
        .join("PawnIOLib.dll");
    path.exists().then_some(path)
}

/// The temperature in a raw `THM_TCON_CUR_TMP` value, in degrees Celsius.
///
/// Split out from the call because it is the whole of the arithmetic and needs no
/// driver to be tested: the value is a 11-bit field in eighths of a degree, and
/// the offset is the one the silicon states in its own flags.
pub(crate) fn celsius_from_smn(raw: u32) -> Option<f32> {
    if raw == 0 || raw == u32::MAX {
        // An unreadable register answers one of these two on some machines, and
        // neither is a temperature.
        return None;
    }
    let field = (raw >> 21) & 0x7FF;
    if field == 0 {
        return None;
    }
    let mut celsius = field as f32 * 0.125;
    let offset_applied = (raw & TEMP_RANGE_SEL) != 0 || (raw & TEMP_TJ_SEL) == TEMP_TJ_SEL;
    if offset_applied {
        celsius -= OFFSET_CELSIUS;
    }
    Some(celsius)
}

/// The range check the rest of the app insists on, for a fractional value.
fn usable_celsius_f32(value: f32) -> Option<f32> {
    let rounded = value.round();
    usable_celsius(rounded as i32).map(|_| rounded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raw value built the way the silicon lays it out.
    fn raw(field: u32, offset_applied: bool) -> u32 {
        let mut value = field << 21;
        if offset_applied {
            value |= TEMP_RANGE_SEL;
        }
        value
    }

    #[test]
    fn the_field_is_eighths_of_a_degree_without_the_offset() {
        // 63.5 °C is field 508 at 0.125 °C a step.
        assert_eq!(celsius_from_smn(raw(508, false)), Some(63.5));
        assert_eq!(celsius_from_smn(raw(48, false)), Some(6.0));
    }

    #[test]
    fn the_offset_is_subtracted_when_the_silicon_says_it_applied_it() {
        // 112.5 − 49 = 63.5, which is what a Zen part over 45 °C reports.
        assert_eq!(celsius_from_smn(raw(900, true)), Some(63.5));
        // The newer parts say it with TJ_SEL instead of RANGE_SEL.
        let mut newer = 900_u32 << 21;
        newer |= TEMP_TJ_SEL;
        assert_eq!(celsius_from_smn(newer), Some(63.5));
    }

    #[test]
    fn a_register_that_answered_nothing_is_not_a_temperature() {
        assert_eq!(celsius_from_smn(0), None);
        assert_eq!(celsius_from_smn(u32::MAX), None);
        // A zero field is a zero field, whatever the flags say.
        assert_eq!(celsius_from_smn(TEMP_RANGE_SEL), None);
    }

    #[test]
    fn an_impossible_reading_is_dropped_by_the_same_range_as_everywhere_else() {
        // 2047 eighths is 256 °C, past boiling: refused.
        assert_eq!(usable_celsius_f32(255.875), None);
        assert_eq!(usable_celsius_f32(63.5), Some(64.0));
    }

    #[test]
    #[ignore = "asks the driver and the processor on this machine"]
    fn live_processor_temperature() {
        match Cpu::open() {
            Some(cpu) => {
                let temperature = cpu.temperature();
                println!("LIVE cpu temperature={temperature:?}");
                if let Some(value) = temperature {
                    assert!((0.0..=150.0).contains(&value), "{value} is not a chip");
                }
            }
            None => println!("LIVE no PawnIO driver or no AMD processor"),
        }
    }
}

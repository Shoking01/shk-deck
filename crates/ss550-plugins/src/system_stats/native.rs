//! The sources this build reads itself, with no monitoring program in between.
//!
//! A Radeon's temperature is published by AMD's own library, which the graphics
//! driver installs; the crate that calls it (`ss550-sensors`) is the workspace's
//! declared `unsafe` exception. Everything this module exposes is still a plain
//! `Option`: no handle is leaked to the caller, and a machine without a Radeon
//! simply answers nothing.
//!
//! The handle is opened once per plugin and kept: creating it is a handshake
//! with the driver, not something to do every two seconds.

#[cfg(windows)]
use std::sync::Mutex;

#[cfg(windows)]
use ss550_sensors::{Cpu, Gpu};

#[cfg(windows)]
static GPU: Mutex<Option<Option<Gpu>>> = Mutex::new(None);

#[cfg(windows)]
static CPU: Mutex<Option<Option<Cpu>>> = Mutex::new(None);

/// The card's edge temperature in whole degrees, from AMD's library.
///
/// The first call opens the library; a machine without one is remembered as
/// absent, so a missing driver costs one failed attempt and not one per sample.
#[cfg(windows)]
pub(crate) fn gpu_temperature() -> Option<f32> {
    let mut guard = GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .get_or_insert_with(Gpu::open)
        .as_ref()
        .and_then(Gpu::temperature)
}

/// The processor's package temperature, from PawnIO's driver.
///
/// The driver refuses a process that is not elevated — `E_ACCESSDENIED`, by
/// design, because it exposes kernel primitives — so this answers `None` from an
/// ordinary start and the rest of the chain takes over. When the app itself runs
/// elevated, the value arrives with no other program involved.
#[cfg(windows)]
pub(crate) fn cpu_temperature() -> Option<f32> {
    // The helper first: it is the process that holds the privilege, so asking it
    // keeps this one unprivileged. It is not running at logon any more — it is
    // started here, on demand, and lives only while the app keeps asking.
    if let Some(celsius) = ss550_sensors::helper::cpu_temperature() {
        return Some(celsius);
    }
    ss550_sensors::helper::ensure_running();
    let mut guard = CPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .get_or_insert_with(Cpu::open)
        .as_ref()
        .and_then(Cpu::temperature)
}

/// Whether the process can open the sensor driver at all.
///
/// The window reports this so a user who elevated the app knows the difference
/// between "no sensor" and "not allowed to ask".
#[cfg(windows)]
pub(crate) fn cpu_driver_available() -> bool {
    ss550_sensors::helper::is_running() || {
        let mut guard = CPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.get_or_insert_with(Cpu::open).is_some()
    }
}

#[cfg(not(windows))]
pub(crate) fn gpu_temperature() -> Option<f32> {
    None
}

#[cfg(not(windows))]
pub(crate) fn cpu_temperature() -> Option<f32> {
    None
}

#[cfg(not(windows))]
pub(crate) fn cpu_driver_available() -> bool {
    false
}

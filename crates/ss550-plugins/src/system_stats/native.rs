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
use ss550_sensors::Gpu;

#[cfg(windows)]
static GPU: Mutex<Option<Option<Gpu>>> = Mutex::new(None);

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

#[cfg(not(windows))]
pub(crate) fn gpu_temperature() -> Option<f32> {
    None
}

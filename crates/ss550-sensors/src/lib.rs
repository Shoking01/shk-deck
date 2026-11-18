//! Native hardware sensors, without a monitoring application in the middle.
//!
//! The app already reads load, memory and free space itself. What it used to
//! borrow a whole other program for is the two temperatures, and this crate is
//! the answer to that:
//!
//! - **The graphics card**: AMD's own display library, `atiadlxx.dll`, is
//!   installed by the graphics driver on every machine with a Radeon in it. It
//!   reports the card's edge temperature, its activity and its clocks, and it
//!   needs no driver of ours, no administrator rights and no extra program. The
//!   library is loaded at run time and never redistributed.
//! - **The processor**: a Ryzen's temperature lives behind privileged registers,
//!   so it needs a kernel driver. [PawnIO](https://pawnio.eu/) is that driver:
//!   signed, GPL-2.0, installed once and already present on a machine that has
//!   run LibreHardwareMonitor. This crate talks to it through the four functions
//!   of `PawnIOLib.dll` and an AMD module blob (LGPL-2.1, shipped with its
//!   notices), and never writes a register.
//!
//! ```
//! use ss550_sensors::Gpu;
//!
//! // `None` on a machine whose driver does not install ADL, or without a
//! // Radeon: a sensor that is not there is not an error, it is an absence.
//! if let Some(gpu) = Gpu::open() {
//!     let _ = gpu.temperature();
//! }
//! ```
//!
//! Every call is read-only, and every failure is an `Option`, never a panic:
//! a sensor that does not answer must cost a dash on a key and nothing else.

#![warn(missing_docs)]

#[cfg(windows)]
mod adl;
#[cfg(windows)]
mod pawnio;

#[cfg(windows)]
pub use adl::Gpu;
#[cfg(windows)]
pub use pawnio::Cpu;

/// The edge temperature of a Radeon, in whole degrees.
///
/// The library reports an integer, and the range is the one the rest of the app
/// insists on: a sensor answering zero, a negative or anything past boiling
/// water is a sensor with nothing to say.
pub(crate) fn usable_celsius(raw: i32) -> Option<f32> {
    let value = raw as f32;
    (value > 0.0 && value <= 150.0).then_some(value)
}

/// A percentage the library reported, clamped to the range a percentage has.
pub(crate) fn usable_percent(raw: i32) -> Option<f32> {
    let value = raw as f32;
    (0.0..=100.0).contains(&value).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::{usable_celsius, usable_percent};

    #[test]
    fn a_sensor_that_answers_nothing_is_not_a_reading() {
        assert_eq!(usable_celsius(0), None);
        assert_eq!(usable_celsius(-5), None);
        assert_eq!(usable_celsius(400), None);
        assert_eq!(usable_celsius(62), Some(62.0));
    }

    #[test]
    fn a_percentage_stays_between_zero_and_a_hundred() {
        assert_eq!(usable_percent(0), Some(0.0));
        assert_eq!(usable_percent(13), Some(13.0));
        assert_eq!(usable_percent(100), Some(100.0));
        assert_eq!(usable_percent(101), None);
        assert_eq!(usable_percent(-1), None);
    }
}

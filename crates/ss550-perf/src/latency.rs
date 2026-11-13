//! How long a press takes to become a running action.
//!
//! The path has three parts, and they are measured apart because they belong to
//! different budgets:
//!
//! 1. **decode** — the deck's own bytes become a `KeyEvent` (real input report,
//!    real driver, wire fake underneath).
//! 2. **dispatch** — the key becomes an action and is handed to the worker.
//!    This is what the window thread pays on the frame the key is pressed.
//! 3. **spawn** — the worker turns the action into a child process.
//!
//! Decode plus dispatch is the number to compare with a press→action budget:
//! it is everything the application itself costs. Spawn happens afterwards, on
//! the worker thread, so it never delays the window.
//!
//! **Nothing here touches hardware.** The reports go into
//! `deck_simulator::Ss550WireFake`, which is the same transport the driver's
//! tests drive.

use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deck_api::{ButtonState, Deck, Transport, TransportError};
use deck_simulator::Ss550WireFake;
use ss550_driver::Ss550Deck;
use ss550_engine::{Action, ActionRunner, ActionStore, CommandSpec, KeyNumber};
use ss550_protocol::INPUT_REPORT_SIZE;

use crate::stats::Summary;

/// The fake behind a lock, so the deck can own its transport.
///
/// Cloning hands out another handle to the **same** fake, which is how the
/// probe queues an input report after `Ss550Deck` has taken the transport.
#[derive(Clone)]
struct SharedWire {
    inner: Arc<Mutex<Ss550WireFake>>,
}

impl Transport for SharedWire {
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .write_report(bytes)
    }

    fn read_input(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .read_input(buf, timeout)
    }

    fn read_control(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .read_control(buf)
    }
}

/// A key-edge input report, as the deck sends one.
///
/// `read_key` uses the **read** numbering; the driver flips it back to the
/// paint numbering before it reports a key, so the probe binds whatever key
/// comes out instead of assuming which one that is.
fn key_report(read_key: u8, pressed: bool) -> Vec<u8> {
    let mut report = vec![0u8; INPUT_REPORT_SIZE];
    report[0..3].copy_from_slice(b"ACK");
    report[5..7].copy_from_slice(b"OK");
    report[9] = read_key;
    report[10] = u8::from(pressed);
    report
}

/// What the probe measured.
pub struct Report {
    /// The key the fake reports, as the engine numbers it.
    pub key: KeyNumber,
    /// From the queued bytes to a decoded `KeyEvent`.
    pub decode: Summary,
    /// From the event to the action being handed to the worker.
    pub dispatch: Summary,
    /// From the action to a started child process.
    pub spawn: Summary,
}

impl Report {
    /// Decode plus dispatch: everything the application costs on a press.
    pub fn host_ms(&self) -> f64 {
        self.decode.median_ms + self.dispatch.median_ms
    }
}

/// A command that starts and leaves at once, so a spawn is measured and not
/// waited on.
fn quick_command() -> CommandSpec {
    let shell = std::env::var_os("ComSpec").unwrap_or_else(|| OsString::from("cmd.exe"));
    CommandSpec::new(shell).with_args([OsString::from("/C"), OsString::from("exit 0")])
}

/// Measures the three parts of a press→action path, `iterations` times each.
pub fn measure(iterations: usize) -> Result<Report, String> {
    if iterations == 0 {
        return Err("iterations must be at least 1".to_string());
    }

    let fake = Ss550WireFake::new();
    let wire = SharedWire {
        inner: Arc::new(Mutex::new(fake)),
    };
    let mut deck = Ss550Deck::over_transport(Box::new(wire.clone()));

    // One press first, to learn which key the driver reports. The driver flips
    // the deck's read numbering into the paint numbering, and this probe must
    // bind the key the application would actually look up.
    queue(&wire, &key_report(1, true));
    let first = deck
        .poll_events(Duration::ZERO)
        .map_err(|error| format!("the fake refused a read: {error}"))?;
    let pressed = first
        .iter()
        .find(|event| event.state() == ButtonState::Pressed)
        .ok_or("the driver decoded no press from a press report")?;
    let index = pressed.key().index();
    let key = KeyNumber::new(u8::try_from(index).map_err(|_| "key index too large")? + 1)
        .map_err(|error| format!("the deck reported an unusable key: {error}"))?;

    let mut store = ActionStore::new();
    store
        .set(
            key,
            Action::Launch {
                program: std::env::var_os("ComSpec")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("cmd.exe")),
                args: vec!["/C".to_string(), "exit 0".to_string()],
            },
        )
        .map_err(|error| format!("the probe's own action was refused: {error}"))?;
    let runner = ActionRunner::new();

    let mut decode = Vec::with_capacity(iterations);
    let mut dispatch = Vec::with_capacity(iterations);
    let mut spawn = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        queue(&wire, &key_report(1, true));

        let started = Instant::now();
        let events = deck
            .poll_events(Duration::ZERO)
            .map_err(|error| format!("the fake refused a read: {error}"))?;
        decode.push(started.elapsed());

        let started = Instant::now();
        let event = events
            .iter()
            .find(|event| event.state() == ButtonState::Pressed)
            .ok_or("the driver decoded no press from a press report")?;
        let number = KeyNumber::new(
            u8::try_from(event.key().index()).map_err(|_| "key index too large")? + 1,
        )
        .map_err(|error| format!("the deck reported an unusable key: {error}"))?;
        let action = store
            .get(number)
            .cloned()
            .ok_or("the probe's own binding disappeared")?;
        runner
            .submit(action)
            .map_err(|error| format!("the runner refused the probe's action: {error}"))?;
        dispatch.push(started.elapsed());

        let started = Instant::now();
        let mut child = quick_command()
            .spawn()
            .map_err(|error| format!("starting the probe's own command failed: {error}"))?;
        let _ = child.wait();
        spawn.push(started.elapsed());
    }

    Ok(Report {
        key,
        decode: Summary::of(&decode).ok_or("no decode samples")?,
        dispatch: Summary::of(&dispatch).ok_or("no dispatch samples")?,
        spawn: Summary::of(&spawn).ok_or("no spawn samples")?,
    })
}

/// Queues one report on the fake.
fn queue(wire: &SharedWire, report: &[u8]) {
    wire.inner
        .lock()
        .expect("the fake's wire is never poisoned")
        .queue_input_report(report);
}

//! The plugins this build ships, and the thread that measures them.
//!
//! A plugin is not a process and not a library loaded at run time: it is a piece
//! of this build that implements [`ss550_plugin_api::Plugin`], which is the
//! whole reason the contract is small. `Catalogue` is what the window lists, and
//! [`Sampler`] is what walks the machine on a thread of its own — reading a
//! sensor is not something a frame can wait for, and it is certainly not
//! something a frame may do.
//!
//! ```
//! use ss550_plugins::Catalogue;
//!
//! let catalogue = Catalogue::built_in();
//! let labels: Vec<&str> = catalogue.cells().iter().map(|cell| cell.label()).collect();
//! assert!(labels.contains(&"CPU"));
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ss550_plugin_api::{CellId, CellSpec, Plugin, PluginId, Reading};

pub mod system_stats;

pub use system_stats::SystemStats;

/// How often the sampler walks the machine.
///
/// Two seconds is the compromise the deck's link asks for: a percentage that
/// moves every second is a JPEG every second per key, and a person reading a key
/// from across a desk cannot use a finer number than this anyway.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

/// The longest the sampler sleeps without checking whether it was told to stop.
const STOP_CHECK: Duration = Duration::from_millis(50);

/// Every plugin this build ships.
///
/// One function, so the catalogue and the sampler can never disagree about what
/// exists: both are built from this list.
fn built_in_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(SystemStats::new())]
}

/// What the window needs to know about the plugins without measuring anything.
///
/// The cells are copied out of the plugins at start: the window lists them, the
/// editor binds them, and the sampler gets the plugins themselves.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    cells: Vec<CellSpec>,
}

impl Catalogue {
    /// The catalogue of this build.
    pub fn built_in() -> Self {
        let cells = built_in_plugins()
            .iter()
            .flat_map(|plugin| plugin.cells().iter().cloned())
            .collect();
        Self { cells }
    }

    /// Every cell, in the order the editor lists them.
    pub fn cells(&self) -> &[CellSpec] {
        &self.cells
    }

    /// The cell a binding names, when this build has it.
    pub fn find(&self, plugin: &str, cell: &str) -> Option<&CellSpec> {
        self.cells
            .iter()
            .find(|spec| spec.plugin().as_str() == plugin && spec.cell().as_str() == cell)
    }

    /// Whether this build can draw a binding at all.
    pub fn knows(&self, plugin: &str, cell: &str) -> bool {
        self.find(plugin, cell).is_some()
    }
}

/// One walk over the machine, merged from every plugin that answered.
///
/// A cell is addressed by its plugin as well as its own id, because two plugins
/// are free to name a cell `temp` and neither is wrong.
#[derive(Debug, Clone, Default)]
pub struct Samples {
    at: Option<Instant>,
    readings: BTreeMap<(PluginId, CellId), Reading>,
}

impl Samples {
    /// An empty set of readings.
    pub fn new() -> Self {
        Self::default()
    }

    /// What one cell measured, when it was measured at all.
    pub fn get(&self, plugin: &PluginId, cell: &CellId) -> Option<&Reading> {
        self.readings.get(&(plugin.clone(), cell.clone()))
    }

    /// The same lookup, for the caller that holds the ids as text.
    pub fn get_named(&self, plugin: &str, cell: &str) -> Option<&Reading> {
        self.readings.iter().find_map(|((owner, id), reading)| {
            (owner.as_str() == plugin && id.as_str() == cell).then_some(reading)
        })
    }

    /// How many cells were measured.
    pub fn len(&self) -> usize {
        self.readings.len()
    }

    /// Whether nothing was measured.
    pub fn is_empty(&self) -> bool {
        self.readings.is_empty()
    }

    /// When the walk started.
    pub fn taken_at(&self) -> Option<Instant> {
        self.at
    }
}

/// Measures every plugin on a thread of its own.
///
/// The thread is deliberately dumb: measure, hand the answer over, sleep. The
/// answer is handed over on a one-slot channel and dropped when nobody has read
/// the last one, so a window that is busy — or hidden — never makes the sampler
/// queue minutes of stale readings.
pub struct Sampler {
    readings: Receiver<Samples>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Sampler {
    /// Starts the sampler, measuring every plugin at `interval`.
    pub fn start(interval: Duration) -> Self {
        let (sender, readings) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));

        let thread = std::thread::Builder::new()
            .name("plugin-sampler".to_string())
            .spawn({
                let stop = Arc::clone(&stop);
                move || run(interval, sender, stop)
            });

        match thread {
            Ok(thread) => Self {
                readings,
                stop,
                thread: Some(thread),
            },
            Err(_) => {
                // No thread is a quiet failure, not a broken window: the keys
                // bound to a plugin keep the reading they had, and the editor
                // still lists the cells.
                Self {
                    readings,
                    stop,
                    thread: None,
                }
            }
        }
    }

    /// Whether the thread is running.
    pub fn is_running(&self) -> bool {
        self.thread.is_some()
    }

    /// Takes the newest walk, when it has not been taken already.
    pub fn try_recv(&self) -> Option<Samples> {
        self.readings.try_recv().ok()
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The sampler's thread: warm up, then measure until told to stop.
fn run(interval: Duration, sender: SyncSender<Samples>, stop: Arc<AtomicBool>) {
    let mut plugins = built_in_plugins();
    // The first reading of a usage counter is meaningless — a counter needs two
    // moments to have a rate — so the warm-up happens once, here, instead of
    // making the first two samples of every session read zero.
    for plugin in &mut plugins {
        let _ = plugin.sample();
    }
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);

    while !stop.load(Ordering::SeqCst) {
        let mut merged = Samples {
            at: Some(Instant::now()),
            readings: BTreeMap::new(),
        };
        for plugin in &mut plugins {
            let snapshot = plugin.sample();
            for (cell, reading) in snapshot.iter() {
                merged
                    .readings
                    .insert((plugin.id().clone(), cell.clone()), reading.clone());
            }
        }

        match sender.try_send(merged) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }

        // Sleep in steps, so a window that closes does not wait two seconds for
        // a thread it just told to stop.
        let deadline = Instant::now() + interval;
        while Instant::now() < deadline {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(STOP_CHECK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_lists_the_cells_the_system_stats_plugin_promises() {
        let catalogue = Catalogue::built_in();
        let ids: Vec<(&str, &str)> = catalogue
            .cells()
            .iter()
            .map(|cell| (cell.plugin().as_str(), cell.cell().as_str()))
            .collect();

        assert_eq!(
            ids,
            [
                ("system-stats", "cpu.usage"),
                ("system-stats", "cpu.temp"),
                ("system-stats", "gpu.usage"),
                ("system-stats", "gpu.temp"),
                ("system-stats", "ram.usage"),
                ("system-stats", "disk.free"),
            ]
        );
        assert!(catalogue.knows("system-stats", "disk.free"));
        assert!(!catalogue.knows("system-stats", "disk.used"));
        assert!(!catalogue.knows("other-plugin", "cpu.usage"));
        assert!(catalogue.find("system-stats", "cpu.temp").is_some());
    }

    #[test]
    fn every_label_fits_the_band_the_window_draws_it_in() {
        // The face draws the label at a size that holds about six characters;
        // a longer one is a plugin writing a sentence where a name belongs.
        for cell in Catalogue::built_in().cells() {
            assert!(
                cell.label().chars().count() <= 6,
                "{} is too long for a key: {:?}",
                cell.cell(),
                cell.label()
            );
            assert!(!cell.hint().is_empty(), "{} has no hint", cell.cell());
        }
    }

    #[test]
    fn a_sample_is_addressed_by_plugin_and_cell() {
        let plugin = PluginId::parse("system-stats").expect("a usable id");
        let cell = CellId::parse("cpu.usage").expect("a usable id");
        let mut samples = Samples::new();
        samples
            .readings
            .insert((plugin.clone(), cell.clone()), Reading::percent(12.0));

        assert_eq!(samples.get(&plugin, &cell).map(Reading::text), Some("12%"));
        assert_eq!(samples.get_named("system-stats", "cpu.usage").map(Reading::text), Some("12%"));
        assert!(samples.get_named("system-stats", "gpu.usage").is_none());
        assert!(samples.get_named("other", "cpu.usage").is_none());
        assert_eq!(samples.len(), 1);
        assert!(!samples.is_empty());
        assert!(Samples::new().is_empty());
    }

    #[test]
    fn the_sampler_answers_with_a_reading_for_every_cell_and_stops() {
        let sampler = Sampler::start(Duration::from_millis(50));
        assert!(sampler.is_running());

        // The first walk happens before the first sleep, so a short wait is
        // enough — and the wait is bounded, so a broken thread fails the test
        // instead of hanging it.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut taken = None;
        while Instant::now() < deadline {
            if let Some(samples) = sampler.try_recv() {
                taken = Some(samples);
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let samples = taken.expect("the sampler answered");
        assert!(samples.taken_at().is_some());
        for cell in Catalogue::built_in().cells() {
            let reading = samples
                .get_named(cell.plugin().as_str(), cell.cell().as_str())
                .unwrap_or_else(|| panic!("{} was not measured", cell.cell()));
            // A machine may answer "no sensor" for a temperature; it may never
            // answer with nothing at all, which is what a missing cell means.
            assert!(!reading.text().is_empty());
        }

        // Dropping the sampler joins the thread; the test passes only if that
        // returns instead of sleeping for the interval.
        let started = Instant::now();
        drop(sampler);
        assert!(
            started.elapsed() < SAMPLE_INTERVAL,
            "a stopped sampler waited for its interval"
        );
    }
}

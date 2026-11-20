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

use ss550_plugin_api::{Cadence, CellId, CellSpec, Plugin, PluginId, Reading};

pub(crate) mod clock;
pub(crate) mod media;
pub mod system_stats;

pub use system_stats::{temperature_hint, SystemStats};

/// How often the sampler walks the machine.
///
/// Two seconds is what measuring a machine costs: one pass over the sensors, the
/// processor's temperature through a helper, the card's through a driver. A
/// plugin that only reads a value the process already has — a clock — does not
/// ride this walk: it says [`Cadence::Own`] and gets a thread of its own, so a
/// second hand ticks every second while the machine keeps its own pace.
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

/// How many samples may wait for the window.
///
/// One slot was enough when every sample was a whole walk: the next one replaced
/// it. With a clock on its own cadence a full slot would drop a *second*, so the
/// channel holds a few — and still drops when the window has been stuck for
/// seconds, because a window that far behind does not want a backlog either.
const SLOTS: usize = 4;

/// The longest the sampler sleeps without checking whether it was told to stop.
const STOP_CHECK: Duration = Duration::from_millis(50);

/// When the next walk is due.
///
/// The sampler must not sleep *for* the interval: that adds the walk's own time
/// to every cycle, so the phase slips and — at a one-second interval — a clock's
/// seconds start skipping and repeating. The deadline is held here and moved on
/// by exactly one interval, so neither a walk that took longer nor a wake-up the
/// system delayed can push the next one late.
struct Schedule {
    next: Instant,
    interval: Duration,
}

impl Schedule {
    /// A schedule whose first walk is due at once.
    fn new(interval: Duration) -> Self {
        Self {
            next: Instant::now(),
            interval,
        }
    }

    /// Sleeps until the next walk is due, in short steps so a stop is answered
    /// promptly. Returns `false` when it was told to stop first.
    fn wait(&mut self, stop: &AtomicBool) -> bool {
        loop {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            let now = Instant::now();
            if now >= self.next {
                break;
            }
            std::thread::sleep(STOP_CHECK.min(self.next - now));
        }

        // The next walk is one interval after the slot this one occupied. A walk
        // that overran its own slot leaves that deadline in the past, and the
        // schedule starts again from now: the samples it ate are gone, and a
        // burst of walks catching up would spend the link on old numbers.
        let next = self.next + self.interval;
        self.next = match next > Instant::now() {
            true => next,
            false => Instant::now() + self.interval,
        };
        true
    }
}

/// How long a stopped sampler is waited for before it is left behind.
///
/// A sample can be inside a WMI query or a disk refresh when it is told to stop,
/// and those calls take no timeout of their own. The stop flag is honoured
/// between samples, so the wait is bounded here at all: a window that is closing
/// must not hang on another thread's provider. The thread owns everything it
/// touches, so letting it finish alone is safe.
const STOP_TIMEOUT: Duration = Duration::from_millis(500);

/// Every plugin this build ships.
///
/// Two lists, and a test holds them together
/// (`the_catalogue_lists_exactly_what_the_plugin_measures`): the catalogue must
/// **not** build a plugin — building one reads the machine, and the catalogue is
/// built while the window opens — while the sampler must build every plugin it
/// measures. [`built_in_descriptions`] is the first list, this is the second.
fn built_in_plugins() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(SystemStats::new()),
        Box::new(clock::Clock::new()),
        Box::new(media::NowPlaying::new()),
    ]
}

/// One plugin as the window sees it before anything is measured: what it calls
/// itself, and the cells it offers.
///
/// The palette needs the name — one draggable row per plugin is what a person
/// recognises — and the editor needs the cells. Neither may build a plugin.
#[derive(Debug, Clone)]
pub struct PluginGroup {
    id: String,
    name: &'static str,
    cells: Vec<CellSpec>,
}

impl PluginGroup {
    /// The plugin's stable id, e.g. `clock`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The name a person reads, e.g. `Clock`.
    pub fn name(&self) -> &str {
        self.name
    }

    /// The cells this plugin offers, in the order the editor lists them.
    pub fn cells(&self) -> &[CellSpec] {
        &self.cells
    }
}

/// Every plugin this build ships, named, without building one.
///
/// This is what the window lists, so it must stay free of I/O: a plugin's
/// `describe` is a table of names, not a measurement. The id is read from the
/// cells themselves — a plugin states it once, in the cells it hands out.
fn built_in_descriptions() -> Vec<PluginGroup> {
    [
        (system_stats::NAME, SystemStats::describe()),
        (clock::NAME, clock::Clock::describe()),
        (media::NAME, media::NowPlaying::describe()),
    ]
    .into_iter()
    .filter_map(|(name, cells)| {
        let id = cells.first()?.plugin().as_str().to_string();
        Some(PluginGroup { id, name, cells })
    })
    .collect()
}

/// What the window needs to know about the plugins without measuring anything.
///
/// The cells are copied out of the plugins at start: the window lists them, the
/// editor binds them, and the sampler gets the plugins themselves.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    groups: Vec<PluginGroup>,
    cells: Vec<CellSpec>,
}

impl Catalogue {
    /// The catalogue of this build.
    ///
    /// Built from what each plugin *says* it measures, never from a plugin that
    /// has been constructed: this runs while the window is being built, and
    /// constructing one reads the machine. That work belongs to the sampler's
    /// thread, and `the_catalogue_lists_exactly_what_the_plugin_measures` holds
    /// the two lists together.
    pub fn built_in() -> Self {
        let groups = built_in_descriptions();
        let cells = groups
            .iter()
            .flat_map(|group| group.cells.iter().cloned())
            .collect();
        Self { groups, cells }
    }

    /// Every plugin this build ships, in the order the palette lists them.
    ///
    /// One row per plugin, not per cell: a person drags "Clock", not "the
    /// seconds of the clock".
    pub fn groups(&self) -> &[PluginGroup] {
        &self.groups
    }

    /// The cells of one plugin, or an empty slice when this build has no such
    /// plugin.
    pub fn cells_of(&self, plugin: &str) -> &[CellSpec] {
        self.groups
            .iter()
            .find(|group| group.id() == plugin)
            .map_or(&[], |group| group.cells())
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

    /// Records what one cell measured.
    ///
    /// The sampler builds a walk this way, and so does any caller that already
    /// has readings in hand — a replay, a test, a window that was told the
    /// numbers by something else.
    pub fn set(&mut self, plugin: PluginId, cell: CellId, reading: Reading) {
        self.readings.insert((plugin, cell), reading);
    }

    /// The same lookup, for the caller that holds the ids as text.
    ///
    /// The two strings are parsed into the ids the map is keyed by, so this is
    /// the one lookup with one spelling: an id the contract would refuse is not a
    /// cell this build measured, and answers `None` like any other absent cell.
    pub fn get_named(&self, plugin: &str, cell: &str) -> Option<&Reading> {
        let plugin = PluginId::parse(plugin).ok()?;
        let cell = CellId::parse(cell).ok()?;
        self.get(&plugin, &cell)
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

    /// Every cell in the set, as its plugin, its cell and what it measured.
    ///
    /// The window records a cell's history from this, so a cell is recorded when
    /// it was actually measured — not once per message, which would give a
    /// sensor's graph two points for the same number whenever the clock's faster
    /// cadence carried it.
    pub fn iter(&self) -> impl Iterator<Item = (&PluginId, &CellId, &Reading)> {
        self.readings
            .iter()
            .map(|((plugin, cell), reading)| (plugin, cell, reading))
    }

    /// Folds another sample into this one.
    ///
    /// Each cadence sends its own message — a clock's second is not a machine
    /// walk — so a window that replaced its readings with the newest message
    /// would blank every sensor key for a second. `at` is the newer of the two,
    /// because the set is the newest reading of each cell, not the newest walk.
    pub fn merge(&mut self, other: Samples) {
        if other.at > self.at {
            self.at = other.at;
        }
        self.readings.extend(other.readings);
    }
}

/// Measures every plugin on a thread of its own.
///
/// The threads are deliberately dumb: measure, hand the answer over, wait. The
/// answer goes on a channel with a few slots and is dropped when the window has
/// not read them, so a window that is busy — or hidden — never makes the sampler
/// queue minutes of stale readings.
pub struct Sampler {
    readings: Receiver<Samples>,
    stop: Arc<AtomicBool>,
    /// One thread for the machine walk, and one for each cadence a plugin asked
    /// for. Empty when no thread could start.
    threads: Vec<JoinHandle<()>>,
}

impl Sampler {
    /// Starts the sampler: the machine walk at `interval`, and every plugin that
    /// asked for its own cadence at the interval it named.
    pub fn start(interval: Duration) -> Self {
        let (sender, readings) = mpsc::sync_channel(SLOTS);
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        // The plugins are split by the cadence they asked for before any thread
        // starts: a plugin is measured by exactly one of them, and which one is
        // the plugin's own statement, not a rule here.
        let mut machine = Vec::new();
        let mut own: BTreeMap<Duration, Vec<Box<dyn Plugin>>> = BTreeMap::new();
        for plugin in built_in_plugins() {
            match plugin.cadence() {
                Cadence::Machine => machine.push(plugin),
                Cadence::Own(every) => own.entry(every).or_default().push(plugin),
            }
        }

        let walk = std::thread::Builder::new()
            .name("plugin-sampler".to_string())
            .spawn({
                let sender = sender.clone();
                let stop = Arc::clone(&stop);
                move || run(interval, machine, sender, stop)
            });
        if let Ok(thread) = walk {
            threads.push(thread);
        }

        for (every, plugins) in own {
            let own_thread = std::thread::Builder::new()
                .name(format!("plugin-own-{}", every.as_millis()))
                .spawn({
                    let sender = sender.clone();
                    let stop = Arc::clone(&stop);
                    move || run_own(every, plugins, sender, stop)
                });
            if let Ok(thread) = own_thread {
                threads.push(thread);
            }
        }

        // No thread is a quiet failure, not a broken window: the keys bound to a
        // plugin keep the reading they had, and the editor still lists the cells.
        Self {
            readings,
            stop,
            threads,
        }
    }

    /// Whether the sampler was started at all.
    pub fn is_running(&self) -> bool {
        !self.threads.is_empty()
    }

    /// Whether every thread is still alive, rather than merely started.
    ///
    /// [`Sampler::is_running`] answers "was it started"; this answers "is it
    /// still there". A sampler thread that panicked leaves its readings frozen on
    /// their last numbers, and a window that cannot tell the difference shows a
    /// stale number as if it were live. One dead thread is enough: the readings
    /// it owned would never move again.
    pub fn is_alive(&self) -> bool {
        !self.threads.is_empty() && self.threads.iter().all(|thread| !thread.is_finished())
    }

    /// Takes the newest sample, when it has not been taken already.
    pub fn try_recv(&self) -> Option<Samples> {
        self.readings.try_recv().ok()
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Waited for, but not forever: the flag is read between samples, so a
        // thread inside a slow provider has to be given a moment — and then left
        // to finish on its own rather than holding the window open.
        let deadline = Instant::now() + STOP_TIMEOUT;
        for thread in &self.threads {
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(STOP_CHECK);
            }
        }
        for thread in self.threads.drain(..) {
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

/// The sampler's machine walk: warm up, then measure until told to stop.
fn run(
    interval: Duration,
    mut plugins: Vec<Box<dyn Plugin>>,
    sender: SyncSender<Samples>,
    stop: Arc<AtomicBool>,
) {
    // The first reading of a usage counter is meaningless — a counter needs two
    // moments to have a rate — so the warm-up happens once, here, instead of
    // making the first two samples of every session read zero.
    for plugin in &mut plugins {
        let _ = plugin.sample();
    }
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);

    // The loop is the schedule: it holds until a walk is due, and one walk is
    // one slot. A stopped schedule ends the loop, which is the only way out.
    let mut schedule = Schedule::new(interval);
    while schedule.wait(&stop) {
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
    }
}

/// One thread per cadence a plugin asked for: measure, hand over, keep the
/// schedule. Nothing here warms a counter up — a plugin that asks for its own
/// cadence is one that reads a value the process already has.
fn run_own(
    interval: Duration,
    mut plugins: Vec<Box<dyn Plugin>>,
    sender: SyncSender<Samples>,
    stop: Arc<AtomicBool>,
) {
    let mut schedule = Schedule::new(interval);
    while schedule.wait(&stop) {
        let mut own = Samples {
            at: Some(Instant::now()),
            readings: BTreeMap::new(),
        };
        for plugin in &mut plugins {
            let snapshot = plugin.sample();
            for (cell, reading) in snapshot.iter() {
                own.readings
                    .insert((plugin.id().clone(), cell.clone()), reading.clone());
            }
        }

        match sender.try_send(own) {
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ss550_plugin_api::Face;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn the_walk_keeps_its_phase_when_a_sample_takes_time() {
        // A clock is sampled like any other cell, so a schedule that pays for
        // the walk out of the interval makes its seconds slip, skip and repeat.
        // Eight cycles are measured against the schedule's own clock: a sampler
        // that slept *for* the interval would show interval + walk per cycle.
        let interval = Duration::from_millis(60);
        let walk = Duration::from_millis(20);
        let stop = AtomicBool::new(false);
        let mut schedule = Schedule::new(interval);
        assert!(schedule.wait(&stop), "the first walk is due at once");

        // Seven slots, each paying a walk out of its own interval.
        let started = Instant::now();
        for _ in 0..7 {
            std::thread::sleep(walk);
            assert!(schedule.wait(&stop), "the schedule was never stopped");
        }
        let elapsed = started.elapsed();

        let slots = interval * 7;
        assert!(
            elapsed >= slots - Duration::from_millis(5),
            "seven slots came early: {elapsed:?}"
        );
        assert!(
            elapsed < slots + Duration::from_millis(80),
            "the walk was added to the schedule, so its phase is walking away: {elapsed:?}"
        );
    }

    #[test]
    fn a_walk_that_overruns_its_slot_does_not_start_a_burst() {
        // A provider that hangs for several slots must cost the samples it ate,
        // not a burst of walks catching up on deadlines already spent.
        let interval = Duration::from_millis(20);
        let stop = AtomicBool::new(false);
        let mut schedule = Schedule::new(interval);

        assert!(schedule.wait(&stop));
        std::thread::sleep(Duration::from_millis(200));

        let started = Instant::now();
        assert!(schedule.wait(&stop));
        let first = started.elapsed();
        assert!(schedule.wait(&stop));
        let second = started.elapsed();

        assert!(
            first <= Duration::from_millis(5),
            "the first walk after an overrun waited: {first:?}"
        );
        assert!(
            second >= interval - Duration::from_millis(5),
            "the sampler tried to catch up on slots it had lost: {second:?}"
        );
    }

    #[test]
    fn a_stopped_schedule_answers_at_once() {
        let interval = Duration::from_secs(60);
        let stop = AtomicBool::new(false);
        let mut schedule = Schedule::new(interval);
        assert!(schedule.wait(&stop));

        stop.store(true, Ordering::SeqCst);
        let started = Instant::now();
        assert!(!schedule.wait(&stop), "a stopped schedule must say so");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a stopped schedule slept for its whole interval"
        );
    }

    #[test]
    fn the_clocks_cells_are_a_number_and_the_machines_are_a_gauge() {
        // The face is the plugin's statement: a clock's key is a time to read at
        // a glance, and a percentage's label is worth its line. This is what the
        // paint code must not decide by look.
        let catalogue = Catalogue::built_in();

        for cell in catalogue.cells_of("clock") {
            assert_eq!(cell.face(), Face::Number, "{} is not a number", cell.cell());
        }
        for cell in catalogue.cells_of("system-stats") {
            assert_eq!(cell.face(), Face::Gauge, "{} is not a gauge", cell.cell());
        }
    }

    #[test]
    fn the_clock_asks_to_be_read_once_a_second() {
        // The whole point of a cadence: reading the time costs nothing, so it
        // must not be priced by the machine's walk, and a second hand that moved
        // with the sensors would not be a clock.
        let clock = clock::Clock::new();

        assert_eq!(clock.cadence(), Cadence::Own(Duration::from_secs(1)));
    }

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
                ("clock", "hours"),
                ("clock", "minutes"),
                ("clock", "seconds"),
                ("media", "now"),
            ]
        );
        assert!(catalogue.knows("system-stats", "disk.free"));
        assert!(!catalogue.knows("system-stats", "disk.used"));
        assert!(!catalogue.knows("other-plugin", "cpu.usage"));
        assert!(catalogue.find("system-stats", "cpu.temp").is_some());
        // A plugin is one row in the palette, whatever it measures: the drift
        // this pins is a palette with nine rows and a person looking for "Clock".
        let names: Vec<(&str, &str, usize)> = catalogue
            .groups()
            .iter()
            .map(|group| (group.id(), group.name(), group.cells().len()))
            .collect();
        assert_eq!(
            names,
            [
                ("system-stats", "System stats", 6),
                ("clock", "Clock", 3),
                ("media", "Now playing", 1),
            ]
        );
        assert_eq!(catalogue.cells_of("clock").len(), 3);
        assert!(catalogue.cells_of("no-such-plugin").is_empty());
    }

    #[test]
    fn the_catalogue_lists_exactly_what_the_plugin_measures() {
        // The window lists `describe`, the sampler measures `new`: if the two
        // ever diverged, the editor would offer a cell no key could show.
        let described = SystemStats::describe();
        let built = SystemStats::new();
        assert_eq!(described, built.cells().to_vec());
        assert_eq!(
            Catalogue::built_in().cells_of("system-stats"),
            described.as_slice()
        );

        // And the same for the clock, which measures the time and nothing else.
        let described = clock::Clock::describe();
        let built = clock::Clock::new();
        assert_eq!(described, built.cells().to_vec());
        assert_eq!(
            Catalogue::built_in().cells_of("clock"),
            described.as_slice()
        );
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
        assert_eq!(
            samples
                .get_named("system-stats", "cpu.usage")
                .map(Reading::text),
            Some("12%")
        );
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

        // Every cadence hands over its own cells, so the sampler is asked until
        // the whole catalogue has answered once — the machine's walk and the
        // clock's second are two messages, not one.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut collected = Samples::new();
        let mut seen = 0;
        while Instant::now() < deadline && seen < Catalogue::built_in().cells().len() {
            if let Some(samples) = sampler.try_recv() {
                seen += samples.len();
                collected.merge(samples);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(collected.taken_at().is_some());
        for cell in Catalogue::built_in().cells() {
            let reading = collected
                .get_named(cell.plugin().as_str(), cell.cell().as_str())
                .unwrap_or_else(|| panic!("{} was not measured", cell.cell()));
            // A machine may answer "no sensor" for a temperature; it may never
            // answer with nothing at all, which is what a missing cell means.
            assert!(!reading.text().is_empty());
        }

        // Dropping the sampler joins every thread; the test passes only if that
        // returns instead of sleeping for an interval. One of them is the
        // clock's own one-second cadence, which is why the bound is not the
        // machine's interval.
        let started = Instant::now();
        drop(sampler);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a stopped sampler waited for its clocks"
        );
    }

    #[test]
    fn the_sampler_measures_the_clock_on_its_own_cadence() {
        // The machine walk is asked for five times a second here and the clock
        // for once; if they shared a walk the two counts would be equal, which
        // is exactly the coupling this separates.
        let sampler = Sampler::start(Duration::from_millis(200));

        let until = Instant::now() + Duration::from_millis(3200);
        let mut walks = 0;
        let mut seconds = 0;
        while Instant::now() < until {
            if let Some(samples) = sampler.try_recv() {
                if samples.get_named("clock", "seconds").is_some() {
                    seconds += 1;
                }
                if samples.get_named("system-stats", "cpu.usage").is_some() {
                    walks += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(
            (2..=5).contains(&seconds),
            "the clock answered {seconds} times in 3.2 s, not about once a second"
        );
        assert!(
            walks >= 6,
            "the machine walk answered only {walks} times in 3.2 s"
        );
        assert!(
            walks > seconds,
            "the machine walk and the clock moved together: {walks} walks, {seconds} seconds"
        );
    }

    #[test]
    fn a_sample_folds_into_the_one_it_joins() {
        // Two cadences, two messages: the second one must not erase the first,
        // or a clock's tick would blank every sensor key for a second.
        let stats = PluginId::parse("system-stats").expect("a usable id");
        let clock = PluginId::parse("clock").expect("a usable id");
        let usage = CellId::parse("cpu.usage").expect("a usable id");
        let seconds = CellId::parse("seconds").expect("a usable id");

        let mut machine = Samples {
            at: Some(Instant::now()),
            readings: BTreeMap::new(),
        };
        machine
            .readings
            .insert((stats.clone(), usage.clone()), Reading::percent(12.0));
        std::thread::sleep(Duration::from_millis(2));
        let tick_at = Instant::now();
        let mut tick = Samples {
            at: Some(tick_at),
            readings: BTreeMap::new(),
        };
        tick.readings.insert(
            (clock.clone(), seconds.clone()),
            Reading::spelled("07", 7.0),
        );

        machine.merge(tick);

        // `at` is the newest of the two, because the set is the newest reading of
        // each cell rather than one walk.
        assert_eq!(machine.taken_at(), Some(tick_at));
        assert_eq!(
            machine
                .get_named("system-stats", "cpu.usage")
                .map(Reading::text),
            Some("12%"),
            "the sensor was erased by the clock's message"
        );
        assert_eq!(
            machine.get_named("clock", "seconds").map(Reading::text),
            Some("07")
        );
        assert_eq!(machine.len(), 2);
    }
}

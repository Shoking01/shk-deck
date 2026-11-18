//! The first plugin: what this machine is doing right now.
//!
//! Six cells, and each one is a different kind of promise:
//!
//! | cell | what it reads | where |
//! |---|---|---|
//! | `cpu.usage` | every core together | the machine's own counters |
//! | `cpu.temp` | the hottest processor sensor | LibreHardwareMonitor, then ACPI |
//! | `gpu.usage` | the busiest GPU engine | the GPU performance counters |
//! | `gpu.temp` | the hottest card sensor | LibreHardwareMonitor |
//! | `ram.usage` | physical memory in use | the machine's own counters |
//! | `disk.free` | free space on the system disk | the machine's own counters |
//!
//! The temperatures are the two the machine may not answer; see
//! [`thermal`] for why, and what the plugin says when they do not.

mod gpu;
mod lhm;
mod machine;
mod native;
mod thermal;

use ss550_plugin_api::{CellId, CellSpec, Plugin, PluginId, Snapshot};

use machine::Machine;
use thermal::Subject;

/// The plugin's stable id.
pub(crate) const ID: &str = "system-stats";
/// The name a person reads in the editor.
pub(crate) const NAME: &str = "System stats";

/// The cell ids, spelled once.
const CPU_USAGE: &str = "cpu.usage";
const CPU_TEMPERATURE: &str = "cpu.temp";
const GPU_USAGE: &str = "gpu.usage";
const GPU_TEMPERATURE: &str = "gpu.temp";
const RAM_USAGE: &str = "ram.usage";
const DISK_FREE: &str = "disk.free";

/// System stats: processor, graphics card, memory and disk.
pub struct SystemStats {
    id: PluginId,
    cells: Vec<CellSpec>,
    /// The cells again, by name, so a sample does not parse its own ids.
    ids: Cells,
    machine: Machine,
}

/// The cell ids as values, filled once when the plugin is built.
struct Cells {
    cpu_usage: CellId,
    cpu_temperature: CellId,
    gpu_usage: CellId,
    gpu_temperature: CellId,
    ram_usage: CellId,
    disk_free: CellId,
}

impl SystemStats {
    /// The cells this plugin offers, without reading anything.
    ///
    /// The window lists these when it starts, and listing them must not touch
    /// the machine: `System::new_all` walks every counter and every volume, and
    /// doing that while the window is being built is the one thing the sampler
    /// thread exists to keep off the UI thread.
    pub fn describe() -> Vec<CellSpec> {
        // A literal id that the contract would refuse is a programming mistake,
        // not a runtime condition: `the_cells_are_named_the_way_the_contract_spells_them`
        // runs this very function in every test, so these cannot fire.
        let id = PluginId::parse(ID).expect("a literal plugin id the contract accepts");
        vec![
            cell(
                &id,
                CPU_USAGE,
                "CPU",
                "Processor load, every core together",
                [0x38, 0xBD, 0xF8],
            ),
            cell(
                &id,
                CPU_TEMPERATURE,
                "CPU °C",
                "Processor temperature, the hottest core",
                [0xF9, 0x73, 0x16],
            ),
            cell(
                &id,
                GPU_USAGE,
                "GPU",
                "Graphics card load, the busiest engine",
                [0xA7, 0x8B, 0xFA],
            ),
            cell(
                &id,
                GPU_TEMPERATURE,
                "GPU °C",
                "Graphics card temperature",
                [0xFB, 0x71, 0x85],
            ),
            cell(
                &id,
                RAM_USAGE,
                "RAM",
                "Physical memory in use",
                [0x2D, 0xD4, 0xBF],
            ),
            cell(
                &id,
                DISK_FREE,
                "DISK",
                "Free space on the system disk",
                [0xFB, 0xBF, 0x24],
            ),
        ]
    }

    /// Builds the plugin and reads the machine once.
    pub fn new() -> Self {
        let id = PluginId::parse(ID).expect("a literal plugin id the contract accepts");
        // The ids are taken by name, not by position: `describe` decides what the
        // editor lists, and a sample must not depend on the order of that table.
        let ids = Cells {
            cpu_usage: cell_id(CPU_USAGE),
            cpu_temperature: cell_id(CPU_TEMPERATURE),
            gpu_usage: cell_id(GPU_USAGE),
            gpu_temperature: cell_id(GPU_TEMPERATURE),
            ram_usage: cell_id(RAM_USAGE),
            disk_free: cell_id(DISK_FREE),
        };
        Self {
            id,
            cells: Self::describe(),
            ids,
            machine: Machine::new(),
        }
    }
}

impl Default for SystemStats {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for SystemStats {
    fn id(&self) -> &PluginId {
        &self.id
    }

    fn name(&self) -> &str {
        NAME
    }

    fn cells(&self) -> &[CellSpec] {
        &self.cells
    }

    fn sample(&mut self) -> Snapshot {
        let mut snapshot = Snapshot::new();
        snapshot.set(self.ids.cpu_usage.clone(), self.machine.cpu());
        snapshot.set(self.ids.ram_usage.clone(), self.machine.memory());
        snapshot.set(self.ids.disk_free.clone(), self.machine.disk());
        snapshot.set(
            self.ids.cpu_temperature.clone(),
            thermal::temperature(Subject::Cpu),
        );
        snapshot.set(self.ids.gpu_usage.clone(), gpu::usage());
        snapshot.set(
            self.ids.gpu_temperature.clone(),
            thermal::temperature(Subject::Gpu),
        );
        snapshot
    }
}

/// What this machine offers for a temperature, in one sentence.
///
/// The plugin cannot invent a sensor, so when a chip answers nothing the useful
/// half of the answer is *what to run*: the window repeats this after a
/// temperature key is bound and nothing answers.
pub fn temperature_hint() -> &'static str {
    thermal::sources_tried()
}

/// One cell of this plugin.
fn cell(plugin: &PluginId, id: &str, label: &str, hint: &str, accent: [u8; 3]) -> CellSpec {
    CellSpec::new(plugin.clone(), cell_id(id), label, hint, accent)
}

/// A cell id from one of the literals above.
///
/// The only `expect` in this crate's production paths, and it is guarded by
/// `the_cells_are_named_the_way_the_contract_spells_them`, which parses every
/// literal through this function.
fn cell_id(id: &str) -> CellId {
    CellId::parse(id).expect("a literal cell id the contract accepts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cells_are_named_the_way_the_contract_spells_them() {
        // Building the plugin is what validates every literal id: the
        // constructor would panic on a bad one, so this test is the guard for
        // the `expect`s in the table above.
        let plugin = SystemStats::new();
        let ids: Vec<&str> = plugin
            .cells()
            .iter()
            .map(|cell| cell.cell().as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "cpu.usage",
                "cpu.temp",
                "gpu.usage",
                "gpu.temp",
                "ram.usage",
                "disk.free"
            ]
        );
        assert_eq!(plugin.id().as_str(), "system-stats");
        assert_eq!(plugin.name(), "System stats");
        assert!(plugin
            .cells()
            .iter()
            .all(|cell| cell.plugin() == plugin.id()));
    }

    #[test]
    fn a_sample_covers_every_cell_it_lists() {
        let mut plugin = SystemStats::new();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let snapshot = plugin.sample();

        for cell in plugin.cells() {
            let reading = snapshot
                .get(cell.cell())
                .unwrap_or_else(|| panic!("{} was not measured", cell.cell()));
            if reading.reason().is_some() {
                assert_eq!(reading.text(), "—");
            } else {
                assert!(!reading.text().is_empty());
            }
        }
    }

    #[test]
    fn the_two_numbers_a_machine_always_states_are_never_missing() {
        // Load and memory come from counters every Windows machine answers; a
        // dash there would be this plugin's bug, not the machine's silence. The
        // disk is asked only for a face: whether a runner lists one is the
        // runner's business, and a dash with a reason is a valid answer.
        let mut plugin = SystemStats::new();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let snapshot = plugin.sample();
        for cell in [&plugin.ids.cpu_usage, &plugin.ids.ram_usage] {
            let reading = snapshot.get(cell).expect("measured");
            assert!(
                reading.fraction().is_some(),
                "{} was not a percentage: {:?}",
                cell,
                reading.reason()
            );
        }
        let disk = snapshot.get(&plugin.ids.disk_free).expect("measured");
        assert!(!disk.text().is_empty(), "the disk always has a face");
    }
}

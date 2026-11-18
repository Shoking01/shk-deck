//! The numbers Windows answers without asking for permission.
//!
//! Load, memory and free space come from `sysinfo`, which reads the same
//! counters Task Manager reads. Nothing here needs administrator rights, a
//! driver, or a vendor tool: these three are what the machine will always state.

use sysinfo::{Disks, System};

use ss550_plugin_api::Reading;

/// The machine's own counters, kept between samples.
pub(crate) struct Machine {
    system: System,
    disks: Disks,
}

impl Machine {
    /// Reads the machine once.
    ///
    /// The CPU usage needs two moments to become a rate, so one refresh happens
    /// here and the sampler's warm-up does the second: without it the first
    /// sample of every session would read a confident zero.
    pub(crate) fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_cpu_usage();
        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
        }
    }

    /// How busy every core is together, with the number of threads beside it.
    pub(crate) fn cpu(&mut self) -> Reading {
        self.system.refresh_cpu_usage();
        let threads = self.system.cpus().len();
        let busy = self.system.global_cpu_usage();
        Reading::percent(busy).with_detail(format!("{threads} threads"))
    }

    /// How much memory is in use, in percent and in gigabytes.
    pub(crate) fn memory(&mut self) -> Reading {
        self.system.refresh_memory();
        memory_reading(self.system.used_memory(), self.system.total_memory())
    }

    /// How much room is free on the disk Windows is installed on.
    pub(crate) fn disk(&mut self) -> Reading {
        // Refreshing the listed disks, not the list: enumerating storage on
        // every sample is a walk of the machine for a number that only changes
        // when a drive is plugged in.
        self.disks.refresh();
        let drive = system_drive();
        let chosen = self
            .disks
            .list()
            .iter()
            .find(|disk| is_system_disk(&disk.mount_point().to_string_lossy(), &drive))
            // A machine that mounts something else first still has a disk; the
            // largest one is the least surprising answer when the system drive
            // cannot be recognised.
            .or_else(|| {
                self.disks
                    .list()
                    .iter()
                    .max_by_key(|disk| disk.total_space())
            });

        match chosen {
            Some(disk) => disk_reading(disk.available_space(), disk.total_space()),
            None => Reading::unavailable("no disk listed"),
        }
    }
}

/// The drive letter Windows itself runs from, e.g. `C:`.
fn system_drive() -> String {
    std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string())
}

/// Whether a mount point is the system drive's.
///
/// The comparison is case-insensitive and prefix-based because Windows spells
/// the same mount point `C:\`, `c:\` and `C:` depending on who answers.
fn is_system_disk(mount_point: &str, drive: &str) -> bool {
    let mount = mount_point.trim().to_uppercase();
    let drive = drive.trim().to_uppercase();
    !drive.is_empty() && mount.starts_with(&drive)
}

/// One free-space reading, from two numbers.
///
/// Split out from [`Machine::disk`] so the rule — what the percentage is, what
/// the detail line says, and what a disk that states no size gets — is testable
/// without a disk.
fn disk_reading(free: u64, total: u64) -> Reading {
    if total == 0 {
        return Reading::unavailable("no size stated");
    }
    let percent = (free as f64 * 100.0 / total as f64) as f32;
    Reading::percent(percent).with_detail(format!("{:.1} / {:.1} GB free", gb(free), gb(total)))
}

/// One memory reading, from two numbers.
fn memory_reading(used: u64, total: u64) -> Reading {
    if total == 0 {
        return Reading::unavailable("no memory size");
    }
    let percent = (used as f64 * 100.0 / total as f64) as f32;
    Reading::percent(percent).with_detail(format!("{:.1} / {:.1} GB", gb(used), gb(total)))
}

/// Bytes as gigabytes, the way a person reads a spec sheet.
fn gb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_reads_as_a_percentage_with_the_two_sizes_beside_it() {
        let total = 16 * 1024 * 1024 * 1024;
        let used = total / 2;
        let reading = memory_reading(used, total);
        assert_eq!(reading.text(), "50%");
        assert_eq!(reading.detail(), Some("8.0 / 16.0 GB"));
        assert_eq!(reading.fraction(), Some(0.5));
    }

    #[test]
    fn free_space_is_the_share_of_the_disk_that_is_still_available() {
        let total = 512 * 1024 * 1024 * 1024;
        let free = 128 * 1024 * 1024 * 1024;
        let reading = disk_reading(free, total);
        assert_eq!(
            reading.text(),
            "25%",
            "the number is the free space, not the use"
        );
        assert_eq!(reading.detail(), Some("128.0 / 512.0 GB free"));
    }

    #[test]
    fn a_disk_or_a_machine_that_states_no_size_says_so_instead_of_dividing_by_zero() {
        assert_eq!(disk_reading(10, 0).text(), "—");
        assert!(disk_reading(10, 0).reason().is_some());
        assert_eq!(memory_reading(10, 0).text(), "—");
        assert!(memory_reading(10, 0).reason().is_some());
    }

    #[test]
    fn the_system_disk_is_found_whichever_way_windows_spells_it() {
        assert!(is_system_disk("C:\\", "C:"));
        assert!(is_system_disk("c:\\", "C:"));
        assert!(!is_system_disk("D:\\", "C:"));
        assert!(!is_system_disk("C:\\", ""));
    }

    #[test]
    fn the_live_machine_answers_load_memory_and_free_space() {
        // The real counters, read once: this is the one test that can tell
        // whether the provider is wired at all. It asserts the shape and not the
        // numbers, because the numbers are this machine's business.
        let mut machine = Machine::new();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);

        let cpu = machine.cpu();
        assert!(cpu.fraction().is_some(), "load is a percentage");
        let memory = machine.memory();
        assert!(memory.fraction().is_some(), "memory is a percentage");
        let disk = machine.disk();
        assert!(
            disk.fraction().is_some(),
            "a machine with no readable disk is not a machine this test runs on: {:?}",
            disk.reason()
        );
    }
}

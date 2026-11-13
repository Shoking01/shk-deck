//! What the controller costs while it is doing nothing.
//!
//! Task Manager and `ps` answer this by eye; this does the same thing with
//! numbers: it watches one process for a fixed window and records its resident
//! memory and its CPU share on every sample. The app is not started here — the
//! measurement is of the instance the user is running, so nothing has to be
//! killed and no second controller is created.
//!
//! **CPU is reported the way sysinfo computes it**: a percentage of one core,
//! so a process that keeps one core busy reads 100 % and one that uses several
//! can read above 100 %. That is the stricter reading of a "< 1 %" budget — a
//! process under 1 % by this measure is also under 1 % of the whole machine.

use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// One reading of the process.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// The CPU share, as a percentage of one core.
    pub cpu_percent: f32,
    /// Resident memory, in bytes.
    pub rss_bytes: u64,
}

/// Every reading taken, in order.
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// The samples, oldest first.
    pub samples: Vec<Sample>,
}

impl Report {
    /// The average CPU share, as a percentage of one core.
    pub fn mean_cpu(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let total: f64 = self
            .samples
            .iter()
            .map(|sample| f64::from(sample.cpu_percent))
            .sum();
        total / self.samples.len() as f64
    }

    /// The busiest sample, as a percentage of one core.
    pub fn max_cpu(&self) -> f32 {
        self.samples
            .iter()
            .map(|sample| sample.cpu_percent)
            .fold(0.0_f32, f32::max)
    }

    /// The largest resident set seen, in bytes.
    pub fn max_rss(&self) -> u64 {
        self.samples
            .iter()
            .map(|sample| sample.rss_bytes)
            .max()
            .unwrap_or(0)
    }

    /// The smallest resident set seen, in bytes.
    pub fn min_rss(&self) -> u64 {
        self.samples
            .iter()
            .map(|sample| sample.rss_bytes)
            .min()
            .unwrap_or(0)
    }

    /// How many samples spent above a budget, and what the highest was.
    ///
    /// One sample can read far above the mean for a reason that has nothing to
    /// do with a steady cost — a timer waking, a device read — so a table that
    /// printed only the mean would hide it and one that printed only the peak
    /// would misreport it. Both are reported.
    pub fn above(&self, percent: f32) -> (usize, f32) {
        let over: Vec<f32> = self
            .samples
            .iter()
            .map(|sample| sample.cpu_percent)
            .filter(|cpu| *cpu > percent)
            .collect();
        let highest = over.iter().copied().fold(0.0_f32, f32::max);
        (over.len(), highest)
    }
}

/// How long the probe waits before the first sample.
///
/// The first refresh has no previous reading to compare against, so its CPU
/// share is meaningless. Waiting one interval gives the second refresh a
/// window to divide by.
const WARM_UP: Duration = Duration::from_millis(500);

/// How often the process is read.
const INTERVAL: Duration = Duration::from_millis(500);

/// Watches `pid` for `window` and returns what it cost.
pub fn measure(pid: u32, window: Duration) -> Result<Report, String> {
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    let kind = ProcessRefreshKind::new().with_cpu().with_memory();

    if system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), kind) == 0 {
        return Err(format!(
            "no process with pid {pid} is running; start the controller first"
        ));
    }
    std::thread::sleep(WARM_UP);

    let started = Instant::now();
    let mut report = Report::default();
    while started.elapsed() < window {
        std::thread::sleep(INTERVAL);
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), kind);
        let Some(process) = system.process(pid) else {
            return Err(format!("pid {pid} left during the measurement"));
        };
        report.samples.push(Sample {
            cpu_percent: process.cpu_usage(),
            rss_bytes: process.memory(),
        });
    }

    if report.samples.is_empty() {
        return Err("the window was too short to take a sample".to_string());
    }
    Ok(report)
}

/// Formats bytes the way a task manager does.
pub fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(samples: &[(f32, u64)]) -> Report {
        Report {
            samples: samples
                .iter()
                .map(|(cpu, rss)| Sample {
                    cpu_percent: *cpu,
                    rss_bytes: *rss,
                })
                .collect(),
        }
    }

    #[test]
    fn an_empty_report_costs_nothing() {
        let empty = Report::default();
        assert_eq!(empty.mean_cpu(), 0.0);
        assert_eq!(empty.max_cpu(), 0.0);
        assert_eq!(empty.max_rss(), 0);
    }

    #[test]
    fn the_average_is_over_the_samples_taken() {
        let measured = report(&[(0.2, 10), (0.4, 20)]);
        assert!((measured.mean_cpu() - 0.3).abs() < 1e-6);
        assert_eq!(measured.max_cpu(), 0.4);
    }

    #[test]
    fn the_memory_range_keeps_both_ends() {
        let measured = report(&[(0.1, 30), (0.1, 10), (0.1, 20)]);
        assert_eq!(measured.min_rss(), 10);
        assert_eq!(measured.max_rss(), 30);
    }

    #[test]
    fn samples_over_a_budget_are_counted_and_the_peak_reported() {
        let measured = report(&[(0.5, 10), (1.4, 10), (0.9, 10), (6.2, 10)]);
        let (over, highest) = measured.above(1.0);
        assert_eq!(over, 2);
        assert_eq!(highest, 6.2);
    }

    #[test]
    fn a_budget_nothing_crosses_counts_nothing() {
        let measured = report(&[(0.5, 10), (0.9, 10)]);
        let (over, highest) = measured.above(1.0);
        assert_eq!(over, 0);
        assert_eq!(highest, 0.0);
    }

    #[test]
    fn bytes_are_reported_in_mebibytes() {
        assert_eq!(mib(1024 * 1024), 1.0);
        assert_eq!(mib(0), 0.0);
    }
}

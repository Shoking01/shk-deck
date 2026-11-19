//! Prints one walk over the machine: what every cell of the system plugin says.
//!
//! Support tool, not part of the window: when a key shows a dash, this answers
//! *which* provider was silent, without opening a debugger.
//!
//! ```text
//! cargo run -p ss550-plugins --example stats_once
//! ```

use ss550_plugin_api::Plugin;
use ss550_plugins::SystemStats;

fn main() {
    let mut plugin = SystemStats::new();
    // The CPU usage needs two moments to become a rate.
    let _ = plugin.sample();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);

    println!("{} ({})", plugin.name(), plugin.id());
    let snapshot = plugin.sample();
    let mut missing_heat = false;
    for cell in plugin.cells() {
        let reading = snapshot.get(cell.cell());
        let value = reading.map_or("<not measured>", |reading| reading.text());
        let detail = reading
            .and_then(|reading| reading.detail().or_else(|| reading.reason()))
            .unwrap_or("");
        if reading.is_some_and(|reading| reading.reason().is_some()) {
            missing_heat = true;
        }
        println!(
            "  {:<10} {:<6} {:<8} {detail}",
            cell.cell().as_str(),
            cell.label(),
            value
        );
    }
    if missing_heat {
        println!(
            "\nA dash is a reading nothing measured. To fill it in: {}",
            ss550_plugins::temperature_hint()
        );
    }
}

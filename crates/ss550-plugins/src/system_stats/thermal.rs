//! How hot the processor and the graphics card are.
//!
//! This is the one reading Windows does not simply answer. There is no built-in
//! sensor API: the numbers live in the chip's own registers, and reading them
//! takes a kernel driver. So the plugin asks the sources that exist, in order,
//! and says nothing rather than guessing when none answers:
//!
//! 1. **LibreHardwareMonitor**, when it is running — it publishes every sensor it
//!    reads in its own WMI namespace, which any program can read without
//!    administrator rights. This is the source that carries both the CPU and an
//!    AMD or NVIDIA GPU.
//! 2. **The ACPI thermal zone** (`MSAcpi_ThermalZoneTemperature`), which is
//!    part of Windows itself. Many desktop boards answer nothing there, and the
//!    number it does answer is a zone of the board rather than a core, so it is
//!    labelled as such.
//!
//! A chip with neither source is reported as a reading that has no number. That
//! is the honest answer: a fabricated `0` on a card that is at 80 °C is worse
//! than a dash.

use ss550_plugin_api::Reading;

/// Which chip a temperature is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subject {
    /// The processor.
    Cpu,
    /// The graphics card.
    Gpu,
}

/// The namespaces that publish every sensor a monitoring tool reads.
///
/// Both tools expose the same WMI class with the same field names, because one
/// grew out of the other. **LibreHardwareMonitor 0.9.5 removed its WMI
/// provider** — a current build creates no namespace at all — so this path is the
/// one older LibreHardwareMonitor and OpenHardwareMonitor still answer on, and
/// [`lhm`] is what a current build is read through.
const MONITOR_NAMESPACES: [(&str, &str); 2] = [
    ("ROOT\\LibreHardwareMonitor", "LHM"),
    ("ROOT\\OpenHardwareMonitor", "OHM"),
];

/// The ACPI thermal zone's label.
const FROM_ACPI: &str = "ACPI";

/// The label for a card read through AMD's own display library.
const FROM_ADL: &str = "ADL";

/// The label for a processor read through PawnIO's driver.
///
/// `SMU` is the processor's own system management unit: the register this reads
/// is its thermal block, so the caption names what measured the number rather
/// than the library that carried it.
const FROM_SMU: &str = "SMU";

/// The temperature of one chip, with its source beside it.
pub(crate) fn temperature(subject: Subject) -> Reading {
    temperature_on_this_platform(subject)
}

/// What this machine offers for a temperature, in one sentence.
///
/// The plugin cannot invent a sensor, so when a chip answers nothing the useful
/// half of the answer is *what was tried*. This is the sentence the window shows
/// after a temperature key is bound and nothing answers.
pub(crate) fn sources_tried() -> &'static str {
    if cfg!(windows) {
        if super::native::cpu_driver_available() {
            // The driver is here and this process may use it: nothing to explain.
            return "the processor is read through PawnIO's driver";
        }
        "open LibreHardwareMonitor and turn on Options → Remote Web Server → Run (port 8085), or keep an older LibreHardwareMonitor / OpenHardwareMonitor running; this machine exposes no ACPI thermal zone to Windows, and PawnIO's driver refuses a process that is not elevated"
    } else {
        "this build has no temperature source on this platform"
    }
}

#[cfg(windows)]
fn temperature_on_this_platform(subject: Subject) -> Reading {
    // The sources that need nothing running come first: AMD's own library for the
    // card, and PawnIO's driver for the processor — the second only when this
    // process may open it, which the driver decides.
    match subject {
        Subject::Gpu => {
            if let Some(celsius) = super::native::gpu_temperature() {
                return Reading::temperature(celsius).with_detail(FROM_ADL);
            }
        }
        Subject::Cpu => {
            if let Some(celsius) = super::native::cpu_temperature() {
                return Reading::temperature(celsius).with_detail(FROM_SMU);
            }
        }
    }
    // A current LibreHardwareMonitor is read through its web server, which is the
    // only interface it still offers; the WMI namespaces below are what an older
    // build and OpenHardwareMonitor answer on.
    if let Some(celsius) = super::lhm::temperature(subject) {
        return Reading::temperature(celsius).with_detail("LHM");
    }
    if let Some((celsius, source)) = from_monitor(subject) {
        return Reading::temperature(celsius).with_detail(source);
    }
    // The thermal zone is a property of the machine, not of a chip: it can
    // answer for the processor but never for the graphics card.
    if subject == Subject::Cpu {
        if let Some(celsius) = acpi_zone() {
            return Reading::temperature(celsius).with_detail(FROM_ACPI);
        }
    }
    Reading::unavailable("no sensor")
}

#[cfg(not(windows))]
fn temperature_on_this_platform(_subject: Subject) -> Reading {
    Reading::unavailable("no sensor")
}

/// The hottest temperature a monitoring tool publishes for `subject`.
///
/// The tools are tried in order, and the first one that answers wins: a machine
/// running both does not pay two queries for the same number.
#[cfg(windows)]
fn from_monitor(subject: Subject) -> Option<(f32, &'static str)> {
    MONITOR_NAMESPACES.iter().find_map(|(namespace, label)| {
        published_temperature(namespace, subject).map(|celsius| (celsius, *label))
    })
}

/// The hottest temperature one tool's namespace reports for `subject`.
#[cfg(windows)]
fn published_temperature(namespace: &str, subject: Subject) -> Option<f32> {
    use serde::Deserialize;

    /// One of the tool's sensors.
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Sensor {
        /// e.g. `CPU Core #1`, `GPU Core`, `CPU Package`.
        name: String,
        /// e.g. `Temperature`, `Voltage`, `Load`.
        sensor_type: Option<String>,
        /// The measurement.
        value: Option<f32>,
    }

    let connection = wmi::WMIConnection::with_namespace_path(namespace).ok()?;
    let rows: Vec<Sensor> = connection
        .raw_query("SELECT Name, SensorType, Value FROM Sensor")
        .ok()?;
    let sensors: Vec<(&str, Option<&str>, Option<f32>)> = rows
        .iter()
        .map(|row| (row.name.as_str(), row.sensor_type.as_deref(), row.value))
        .collect();
    pick_temperature(&sensors, subject)
}

/// The hottest ACPI thermal zone, in Celsius.
#[cfg(windows)]
fn acpi_zone() -> Option<f32> {
    use serde::Deserialize;

    /// One thermal zone of the board.
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Zone {
        /// Tenths of a kelvin.
        current_temperature: u32,
    }

    let connection = wmi::WMIConnection::with_namespace_path("ROOT\\WMI").ok()?;
    let rows: Vec<Zone> = connection
        .raw_query("SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature")
        .ok()?;
    rows.iter()
        .filter_map(|zone| kelvin_tenths_to_celsius(zone.current_temperature))
        .reduce(f32::max)
}

/// One ACPI reading, converted, and refused when it cannot be a chip's heat.
///
/// The zones are tenths of a kelvin. A value outside a range a running machine
/// could plausibly be in is a sensor that answered nothing useful — some boards
/// report a constant `0` — and a dash is better than `-273°C`.
fn kelvin_tenths_to_celsius(raw: u32) -> Option<f32> {
    let celsius = raw as f32 / 10.0 - 273.15;
    ((0.0 < celsius) && celsius <= 150.0).then_some(celsius)
}

/// The hottest temperature sensor that belongs to `subject`.
///
/// A sensor answers only when it says it is a temperature, carries a value, is
/// in a range a running machine could be in, and its name belongs to the chip
/// asked about. "Hottest" and not "first": a processor with four cores has four
/// answers, and the one that matters is the worst of them.
fn pick_temperature(
    sensors: &[(&str, Option<&str>, Option<f32>)],
    subject: Subject,
) -> Option<f32> {
    sensors
        .iter()
        .filter(|(name, kind, value)| {
            let is_temperature = kind.is_some_and(|kind| kind.eq_ignore_ascii_case("temperature"));
            let value = value.filter(|value| value.is_finite() && *value > 0.0 && *value <= 150.0);
            is_temperature && value.is_some() && belongs_to(name, subject)
        })
        .filter_map(|(_, _, value)| *value)
        .reduce(f32::max)
}

/// Whether a sensor's name says it measures `subject`.
///
/// LibreHardwareMonitor names its sensors in plain words: `CPU Package`,
/// `CPU Core #1`, `Core (Tctl/Tdie)`, `CCD1 (Tdie)`, `GPU Core`, `GPU Hot Spot`.
/// The `CCD` and `Tdie` spellings are the AMD processor's own, which is why they
/// are here: a name-matching rule that only looked for "cpu" would leave every
/// AMD chip unanswered.
fn belongs_to(name: &str, subject: Subject) -> bool {
    let name = name.to_ascii_lowercase();
    match subject {
        Subject::Cpu => {
            let processor = ["cpu", "core", "tctl", "tdie", "ccd"]
                .iter()
                .any(|word| name.contains(word));
            processor && !name.contains("gpu")
        }
        Subject::Gpu => name.contains("gpu"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sensor<'a>(
        name: &'a str,
        kind: &'a str,
        value: f32,
    ) -> (&'a str, Option<&'a str>, Option<f32>) {
        (name, Some(kind), Some(value))
    }

    #[test]
    fn an_acpi_zone_in_tenths_of_a_kelvin_is_degrees_celsius() {
        // 3000 tenths of a kelvin is 300 K: 26.85 °C.
        let celsius = kelvin_tenths_to_celsius(3000).expect("a plausible temperature");
        assert!((celsius - 26.85).abs() < 0.01);
    }

    #[test]
    fn a_zone_that_cannot_be_a_chip_answers_nothing() {
        assert_eq!(
            kelvin_tenths_to_celsius(0),
            None,
            "273 °C below zero is no reading"
        );
        assert_eq!(kelvin_tenths_to_celsius(10), None);
        assert_eq!(
            kelvin_tenths_to_celsius(5000),
            None,
            "226 °C until the metal melts"
        );
    }

    #[test]
    fn the_hottest_core_is_the_answer_and_neighbours_are_not() {
        let sensors = [
            sensor("CPU Core #1", "Temperature", 45.0),
            sensor("CPU Core #2", "Temperature", 61.5),
            sensor("CPU Core #1", "Voltage", 1.2),
            sensor("GPU Core", "Temperature", 88.0),
        ];
        assert_eq!(pick_temperature(&sensors, Subject::Cpu), Some(61.5));
        assert_eq!(pick_temperature(&sensors, Subject::Gpu), Some(88.0));
    }

    #[test]
    fn an_amd_processor_answers_through_its_own_spellings() {
        let sensors = [
            sensor("Core (Tctl/Tdie)", "Temperature", 52.0),
            sensor("CCD1 (Tdie)", "Temperature", 57.0),
        ];
        assert_eq!(pick_temperature(&sensors, Subject::Cpu), Some(57.0));
    }

    #[test]
    fn a_sensor_outside_a_plausible_range_is_not_an_answer() {
        let sensors = [
            sensor("CPU Package", "Temperature", 0.0),
            sensor("CPU Core #1", "Temperature", 400.0),
        ];
        assert_eq!(pick_temperature(&sensors, Subject::Cpu), None);
    }

    #[test]
    fn one_chip_never_answers_for_the_other() {
        let sensors = [sensor("GPU Hot Spot", "Temperature", 90.0)];
        assert_eq!(pick_temperature(&sensors, Subject::Cpu), None);
        assert_eq!(pick_temperature(&sensors, Subject::Gpu), Some(90.0));
    }

    #[test]
    fn the_reading_says_where_the_number_came_from() {
        let sensors = [sensor("CPU Package", "Temperature", 44.0)];
        let reading =
            Reading::temperature(pick_temperature(&sensors, Subject::Cpu).expect("a value"))
                .with_detail(MONITOR_NAMESPACES[0].1);
        assert_eq!(reading.text(), "44°C");
        assert_eq!(reading.detail(), Some("LHM"));
    }

    #[test]
    fn both_monitoring_tools_are_read_and_the_newer_one_comes_first() {
        // One is a fork of the other and they publish the same class in
        // different namespaces: a machine with either installed must answer.
        assert_eq!(
            MONITOR_NAMESPACES,
            [
                ("ROOT\\LibreHardwareMonitor", "LHM"),
                ("ROOT\\OpenHardwareMonitor", "OHM"),
            ]
        );
        // What the status line says depends on what this machine is running —
        // the helper, or a monitoring tool — so the test asks only that it says
        // something, never that it says one particular thing.
        assert!(!sources_tried().trim().is_empty());
    }

    #[test]
    fn the_live_chips_answer_or_say_they_have_no_sensor() {
        for subject in [Subject::Cpu, Subject::Gpu] {
            let reading = temperature(subject);
            assert!(!reading.text().is_empty());
            if let Some(reason) = reading.reason() {
                assert_eq!(reason, "no sensor");
            } else {
                assert!(reading.text().ends_with("°C"));
            }
        }
    }
}

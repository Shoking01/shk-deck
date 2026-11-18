//! LibreHardwareMonitor's web server: the interface current versions offer.
//!
//! Until 0.9.4 the tool published its sensors in a WMI namespace. **0.9.5
//! removed that provider** — the current build creates no namespace at all, which
//! is why a machine running it answers nothing through WMI. What it offers
//! instead is a small HTTP server, off by default:
//!
//! ```text
//! Options → Remote Web Server → Run      (port 8085 unless it was changed)
//! ```
//!
//! `GET /data.json` returns the whole sensor tree. A sensor node carries a
//! `Type` (its kind, `Temperature` among them), a formatted `Value` and an
//! **`RawValue`**, which is the number in the unit the tree states — degrees
//! Celsius for a temperature. The formatted value is a string with a unit and a
//! locale, so the raw one is what this reads.
//!
//! Reading it is a plain HTTP GET over loopback with no dependencies of its own:
//! the request is `HTTP/1.0` so the server answers with a length and closes, and
//! the timeouts are short enough that a missing server costs the sampler a
//! couple of hundred milliseconds once every two seconds, not the window.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::thermal::Subject;

/// Where the server listens unless the user moved it.
///
/// The port is the one the tool states in its own `Interface / Port` menu.
const ADDR: &str = "127.0.0.1:8085";

/// How long a connection attempt may take before the source is called absent.
const CONNECT: Duration = Duration::from_millis(200);

/// How long the answer may take once connected.
const READ: Duration = Duration::from_millis(500);

/// The hottest temperature the server reports for `subject`.
pub(crate) fn temperature(subject: Subject) -> Option<f32> {
    let body = fetch()?;
    hottest(&body, subject)
}

/// The sensor tree, or `None` when the server is not there or not answering.
fn fetch() -> Option<String> {
    let addr = ADDR.to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT).ok()?;
    stream.set_read_timeout(Some(READ)).ok()?;
    stream.set_write_timeout(Some(CONNECT)).ok()?;

    // `HTTP/1.0` and `Connection: close`: the body is read to the end of the
    // stream, so there is no chunked encoding to decode for a document this
    // small.
    stream
        .write_all(
            b"GET /data.json HTTP/1.0\r\nHost: 127.0.0.1\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        )
        .ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;

    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let status = head.lines().next()?;
    if !status.contains(" 200 ") {
        return None;
    }
    Some(body.to_string())
}

/// One node of the tree, as the server writes it.
///
/// Only the fields this needs are named; everything else in the document is
/// ignored, so a new version that adds a key does not break the read.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Node {
    /// The sensor's kind on a sensor node, e.g. `Temperature`.
    #[serde(rename = "Type", default)]
    kind: Option<String>,
    /// The number in its own unit; `NaN` arrives as that word, which is why it
    /// is read as a value and converted instead of as a number.
    #[serde(default)]
    raw_value: Option<serde_json::Value>,
    /// On a device node, its identifier: `/amdcpu/0`, `/gpu-amd/0`, …
    #[serde(default)]
    hardware_id: Option<String>,
    /// The node's children.
    #[serde(default)]
    children: Vec<Node>,
}

impl Node {
    /// The number this sensor measured, when it measured one.
    ///
    /// The range is the same one the WMI path refuses readings outside of: a
    /// sensor answering `0` or `400` is a sensor with nothing to say, and a dash
    /// on the key is better than a made-up degree.
    fn measured(&self) -> Option<f32> {
        let value = match self.raw_value.as_ref()? {
            serde_json::Value::Number(number) => number.as_f64()?,
            // `AllowNamedFloatingPointLiterals` on the server side writes `NaN`
            // as a string.
            _ => return None,
        };
        let value = value as f32;
        (value.is_finite() && value > 0.0 && value <= 150.0).then_some(value)
    }
}

/// The hottest temperature under the node for `subject`.
fn hottest(body: &str, subject: Subject) -> Option<f32> {
    let root: Node = serde_json::from_str(body).ok()?;
    let mut best: Option<f32> = None;
    walk(&root, subject, &mut best);
    best
}

/// Visits one node and its children, never leaving the subject's own device.
fn walk(node: &Node, subject: Subject, best: &mut Option<f32>) {
    // A device node names what it is: a processor answers the processor's
    // question and nothing else, so a disk's temperature can never be reported
    // as the chip's.
    if let Some(hardware) = &node.hardware_id {
        if !belongs(hardware, subject) {
            return;
        }
    }
    if node.kind.as_deref() == Some("Temperature") {
        if let Some(value) = node.measured() {
            *best = Some(best.map_or(value, |known| known.max(value)));
        }
    }
    for child in &node.children {
        walk(child, subject, best);
    }
}

/// Whether a device identifier is the device `subject` asks about.
fn belongs(hardware_id: &str, subject: Subject) -> bool {
    let id = hardware_id.to_ascii_lowercase();
    match subject {
        Subject::Cpu => id.starts_with("/amdcpu") || id.starts_with("/intelcpu"),
        Subject::Gpu => {
            id.starts_with("/gpu-amd")
                || id.starts_with("/gpu-nvidia")
                || id.starts_with("/gpu-intel")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One device with a temperature group, as the server writes it.
    fn document(cpu: &[(&str, f32)], gpu: &[(&str, f32)]) -> String {
        let sensors = |rows: &[(&str, f32)]| {
            rows.iter()
                .map(|(name, value)| {
                    format!(
                        r#"{{"id":9,"Text":"{name}","Type":"Temperature","Value":"{value} °C","RawValue":{value},"Children":[]}}"#
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        let cpu_tree = format!(
            r#"{{"id":2,"Text":"AMD Ryzen 5 5600","HardwareId":"/amdcpu/0","Children":[{{"id":3,"Text":"Temperatures","Children":[{}]}}]}}"#,
            sensors(cpu)
        );
        let gpu_tree = format!(
            r#"{{"id":4,"Text":"AMD Radeon RX 6600","HardwareId":"/gpu-amd/0","Children":[{{"id":5,"Text":"Temperatures","Children":[{}]}}]}}"#,
            sensors(gpu)
        );
        let board = r#"{"id":6,"Text":"Nuvoton NCT6798D","HardwareId":"/lpc/nct6798d/0","Children":[{"id":7,"Text":"Temperatures","Children":[{"id":8,"Text":"System","Type":"Temperature","Value":"40.0 °C","RawValue":40.0,"Children":[]}]}]}"#;
        format!(
            r#"{{"id":1,"Text":"Sensor","Children":[{{"id":0,"Text":"DESKTOP","Children":[{cpu_tree},{gpu_tree},{board}]}}]}}"#
        )
    }

    #[test]
    fn the_hottest_core_is_the_cpu_and_the_card_is_the_gpu() {
        let body = document(
            &[("Core (Tctl/Tdie)", 57.0), ("CCD1 (Tdie)", 52.0)],
            &[("GPU Core", 61.0), ("GPU Hot Spot", 74.0)],
        );
        assert_eq!(hottest(&body, Subject::Cpu), Some(57.0));
        assert_eq!(
            hottest(&body, Subject::Gpu),
            Some(74.0),
            "the hottest sensor of the card answers"
        );
    }

    #[test]
    fn a_disk_or_a_board_never_answers_for_a_chip() {
        let body = document(&[], &[]);
        assert_eq!(hottest(&body, Subject::Cpu), None);
        assert_eq!(hottest(&body, Subject::Gpu), None);
    }

    #[test]
    fn nan_and_impossible_numbers_are_not_readings() {
        let body = r#"{"id":1,"Text":"Sensor","Children":[
            {"id":0,"Text":"DESKTOP","Children":[
                {"id":2,"Text":"CPU","HardwareId":"/amdcpu/0","Children":[
                    {"id":3,"Text":"Temperatures","Children":[
                        {"id":4,"Text":"Core #1","Type":"Temperature","RawValue":"NaN","Children":[]},
                        {"id":5,"Text":"Core #2","Type":"Temperature","RawValue":0.0,"Children":[]},
                        {"id":6,"Text":"Core #3","Type":"Temperature","RawValue":400.0,"Children":[]}
                    ]}
                ]}
            ]}
        ]}"#;
        assert_eq!(hottest(body, Subject::Cpu), None);
    }

    #[test]
    fn a_document_that_is_not_the_sensor_tree_answers_nothing() {
        assert_eq!(hottest("", Subject::Cpu), None);
        assert_eq!(hottest("{\"hello\":\"world\"}", Subject::Cpu), None);
        assert_eq!(hottest("<html>404</html>", Subject::Gpu), None);
    }

    #[test]
    fn the_device_identifiers_are_read_the_way_the_tool_writes_them() {
        assert!(belongs("/amdcpu/0", Subject::Cpu));
        assert!(belongs("/intelcpu/0", Subject::Cpu));
        assert!(!belongs("/gpu-amd/0", Subject::Cpu));
        assert!(belongs("/gpu-nvidia/0", Subject::Gpu));
        assert!(belongs("/gpu-intel/0", Subject::Gpu));
        assert!(!belongs("/hdd/0", Subject::Gpu));
        assert!(!belongs("/lpc/nct6798d/0", Subject::Cpu));
    }

    #[test]
    fn a_missing_server_does_not_hold_the_sampler() {
        // This runs against the configured port, which in CI and on any machine
        // without the server open is closed: the call has to return an answer
        // rather than block the sampler for its interval.
        let started = std::time::Instant::now();
        let _ = fetch();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "a missing server must not hold the sampler"
        );
    }
}

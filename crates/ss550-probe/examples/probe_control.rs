//! Does control `SET_REPORT` actually reach the firmware?
//!
//! Reads the input report twice with no command in between. If both reads
//! already contain the version string, that report is a static identity
//! report and a `VER` write proves nothing about the write path.
//!
//! Run: `cargo run -p ss550-probe --example probe_control`

use hidapi::{HidApi, HidDevice};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '.'
            }
        })
        .collect()
}

fn read_once(dev: &HidDevice, label: &str) {
    let mut buf = vec![0u8; 1025];
    buf[0] = 0x00;
    match dev.get_input_report(&mut buf) {
        Ok(n) => println!(
            "{label:<14} bytes={n} ascii={:?}",
            ascii(&buf[..n.min(buf.len())])
        ),
        Err(e) => println!("{label:<14} ERR {e}"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = HidApi::new()?;
    let devices: Vec<_> = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .collect();
    let info = devices
        .iter()
        .find(|d| d.usage_page() == VENDOR_USAGE_PAGE)
        .or_else(|| devices.first())
        .ok_or("device not found")?;
    println!("usage_page=0x{:04x}", info.usage_page());
    let dev = info.open_device(&api)?;

    // Baseline: no command was sent in this process.
    read_once(&dev, "baseline-1");
    std::thread::sleep(std::time::Duration::from_millis(300));
    read_once(&dev, "baseline-2");

    // Now ask for the version over the control path.
    let mut ver = vec![0u8; 1025];
    ver[1..4].copy_from_slice(b"CRT");
    ver[6..9].copy_from_slice(b"VER");
    match dev.send_output_report(&ver) {
        Ok(()) => println!("{:<14} ok", "set_report VER"),
        Err(e) => println!("{:<14} ERR {e}", "set_report VER"),
    }

    std::thread::sleep(std::time::Duration::from_millis(300));
    read_once(&dev, "after-ver-1");
    std::thread::sleep(std::time::Duration::from_millis(300));
    read_once(&dev, "after-ver-2");

    Ok(())
}

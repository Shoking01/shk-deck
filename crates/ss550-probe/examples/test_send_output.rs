//! Minimal test: DIS via `send_output_report` (HidD_SetOutputReport) vs `write`.
//! Run: `cargo run -p ss550-probe --example test_send_output`

use hidapi::HidApi;

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;

fn build_dis() -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..9].copy_from_slice(b"DIS");
    buf
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
    println!(
        "opening usage_page=0x{:04x} path={:?}",
        info.usage_page(),
        info.path()
    );
    let dev = info.open_device(&api)?;
    let dis = build_dis();
    println!("trying send_output_report DIS...");
    match dev.send_output_report(&dis) {
        Ok(()) => println!("send_output_report DIS: OK"),
        Err(e) => println!("send_output_report DIS: ERR {e}"),
    }
    println!("trying write DIS for comparison...");
    match dev.write(&dis) {
        Ok(n) => println!("write DIS: OK wrote={n}"),
        Err(e) => println!("write DIS: ERR {e}"),
    }
    Ok(())
}

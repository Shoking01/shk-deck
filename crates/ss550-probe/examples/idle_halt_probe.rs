//! Does the deck stop servicing the interrupt OUT endpoint after idle time?
//!
//! Hypothesis to test (not assumed): after a successful paint the firmware
//! stops consuming output reports once the host goes quiet, and from then on
//! every write is stuck in `ERROR_IO_PENDING` until the device is power cycled.
//!
//! Sequence:
//!   1. optional settle delay (default 10 s, `--settle <secs>`), because writing
//!      while Windows is still re-enumerating a just-plugged device fails
//!      instantly with `0x1F` and is not a stalled endpoint.
//!   2. `DIS` - if this already fails, the deck is halted right now and no
//!      idle measurement is meaningful (do the 60 s power cycle first).
//!   3. paint white on firmware key 1 through the verified path, so the
//!      transport is known good at t=0.
//!   4. every 30 s, send one harmless `DIS` and print elapsed time. Stop at the
//!      first failure - never retry, each rejected report extends a halt.
//!
//! Outcome tells us which runbook rule is missing:
//!
//! * fails after ~N s of idle: add "keep the endpoint warm" as a rule
//! * survives the full 10 min: the previous halts came from something the host
//!   sent, not from idleness
//!
//! Run: `cargo run -p ss550-probe --example idle_halt_probe`

use hidapi::HidApi;
use std::time::{Duration, Instant};

const VID: u16 = 0x0200;
const PID: u16 = 0x3000;
const VENDOR_USAGE_PAGE: u16 = 0xffa0;

/// How long to stay quiet between two probes.
const IDLE_STEP: Duration = Duration::from_secs(30);
/// Total idle window to cover: 10 minutes.
const IDLE_ROUNDS: u32 = 20;

fn crt(tag: &[u8; 3], fields: &[(usize, u8)]) -> Vec<u8> {
    let mut buf = vec![0u8; 1025];
    buf[1..4].copy_from_slice(b"CRT");
    buf[6..9].copy_from_slice(tag);
    for (off, v) in fields {
        buf[1 + off] = *v;
    }
    buf
}

fn solid_jpeg(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(100 * 100 * 3);
    for _ in 0..100 * 100 {
        rgb.extend_from_slice(&[r, g, b]);
    }
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&rgb, 100, 100, jpeg_encoder::ColorType::Rgb)
        .expect("encode");
    jpeg
}

/// Sends one report; returns `Ok(elapsed)` or the error message.
fn send(dev: &hidapi::HidDevice, pack: &[u8]) -> Result<Duration, String> {
    let start = Instant::now();
    match dev.write(pack) {
        Ok(_) => Ok(start.elapsed()),
        Err(e) => Err(format!("{}ms {e}", start.elapsed().as_millis())),
    }
}

fn paint_white(dev: &hidapi::HidDevice) -> Result<(), String> {
    let jpeg = solid_jpeg(0xff, 0xff, 0xff);
    let bat = crt(
        b"BAT",
        &[
            (10, (jpeg.len() >> 8) as u8),
            (11, (jpeg.len() & 0xff) as u8),
            (12, 1),
        ],
    );
    send(dev, &bat).map_err(|e| format!("BAT: {e}"))?;

    let mut chunk = vec![0u8; 1025];
    chunk[1..1 + jpeg.len()].copy_from_slice(&jpeg);
    send(dev, &chunk).map_err(|e| format!("chunk: {e}"))?;

    send(dev, &crt(b"STP", &[])).map_err(|e| format!("STP: {e}"))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Writing while Windows is still re-enumerating a just-plugged device fails
    // instantly with `0x1F`; that is not a stalled endpoint and must not be
    // measured as one.
    let settle_s: u64 = std::env::args()
        .position(|a| a == "--settle")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    println!("settling {settle_s}s before the first write (re-enumeration guard)");
    std::thread::sleep(Duration::from_secs(settle_s));

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
    let dev = info.open_device(&api)?;
    println!(
        "opened interface={:?} usage_page=0x{:04x} (vendor collection: {})",
        info.interface_number(),
        info.usage_page(),
        info.usage_page() == VENDOR_USAGE_PAGE
    );

    println!("--- t=0: is the transport alive at all? ---");
    if let Err(e) = send(&dev, &crt(b"DIS", &[])) {
        println!("DIS failed: {e}");
        println!("RESULT: deck is already halted. Power cycle (60 s, other port) and rerun.");
        return Ok(());
    }

    println!("--- paint so the transport is known good at t=0 ---");
    if let Err(e) = paint_white(&dev) {
        println!("paint failed: {e}");
        println!("RESULT: transport died during the first paint.");
        return Ok(());
    }
    println!("painted WHITE on firmware key 1 (bottom-left)");
    println!();
    println!(
        "--- going idle: one DIS every {}s, stop at first failure ---",
        IDLE_STEP.as_secs()
    );

    let quiet_since = Instant::now();
    for round in 1..=IDLE_ROUNDS {
        std::thread::sleep(IDLE_STEP);
        match send(&dev, &crt(b"DIS", &[])) {
            Ok(elapsed) => println!(
                "t=+{:>3}s  ok   {}ms",
                quiet_since.elapsed().as_secs(),
                elapsed.as_millis()
            ),
            Err(e) => {
                println!("t=+{:>3}s  ERR  {e}", quiet_since.elapsed().as_secs());
                println!();
                println!(
                    "RESULT: endpoint stopped answering after ~{}s of idle.",
                    round * 30
                );
                println!("The firmware stops servicing interrupt OUT when the host goes quiet.");
                return Ok(());
            }
        }
    }

    println!();
    println!(
        "RESULT: survived {} minutes of idle (DIS still completes). Idleness is NOT the trigger.",
        IDLE_ROUNDS * 30 / 60
    );
    Ok(())
}

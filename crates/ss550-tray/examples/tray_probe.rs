//! A probe that shows the tray icon for a few seconds.
//!
//! Run it with `cargo run -p ss550-tray --example tray_probe`. The icon has
//! to appear in the notification area, and it has to leave again when the
//! probe ends.

use std::time::Duration;

use ss550_tray::TrayHost;

fn main() {
    let tray = TrayHost::spawn("StreamCraft SS550 Controller - probe")
        .expect("the tray icon should start");
    println!("tray started");

    std::thread::sleep(Duration::from_secs(5));

    println!("pending command: {:?}", tray.try_recv());
    drop(tray);
    println!("tray stopped");
}

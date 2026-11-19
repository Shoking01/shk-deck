//! Prints the badge as characters, so a digit can be looked at before it ships.
//!
//! ```text
//! cargo run -p ss550-tray --example icon_preview -- 59
//! ```
//!
//! The taskbar halves this to sixteen pixels, so the printed grid is twice what
//! a person sees; `#` is a lit digit pixel and `.` is the badge.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = std::env::args().nth(1).unwrap_or_else(|| "59".to_string());
    let pixels = ss550_tray::icon::badge_with_digits(&text, [0xff, 0xff, 0xff])
        .ok_or("that is not one to three digits")?;
    let size = ss550_tray::icon::ICON_SIZE as usize;
    for y in 0..size {
        for x in 0..size {
            let offset = (y * size + x) * 4;
            let pixel = &pixels[offset..offset + 4];
            let square = if pixel[3] == 0 {
                ' '
            } else if pixel[..3] == [0xff, 0xff, 0xff] {
                '#'
            } else {
                '.'
            };
            print!("{square}");
        }
        println!();
    }
    Ok(())
}

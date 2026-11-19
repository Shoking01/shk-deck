//! Prints every application a key can be bound to, one per line.
//!
//! Support tool, not part of the window: when the picker does not offer an
//! application the user can see in Explorer's Start Menu, this answers *what the
//! scan found* without opening a debugger.
//!
//! ```text
//! cargo run -p ss550-tray --example apps_once
//! cargo run -p ss550-tray --example apps_once -- calc
//! ```
//!
//! The optional argument filters by name, the same way the picker's search does.

fn main() {
    let query = std::env::args().nth(1).unwrap_or_default().to_lowercase();

    let apps = ss550_tray::installed_apps();
    let mut shown = 0;
    let mut packaged = 0;
    for app in &apps {
        if app.is_packaged() {
            packaged += 1;
        }
        let matches = query.is_empty()
            || app.name().to_lowercase().contains(&query)
            || app
                .target()
                .to_string_lossy()
                .to_lowercase()
                .contains(&query);
        if !matches {
            continue;
        }
        shown += 1;
        let kind = match app.is_packaged() {
            true => "packaged",
            false => "file",
        };
        println!("{:<9} {}", kind, app.name());
        println!("          run: {}", app.program().display());
        if !app.arguments().is_empty() {
            println!("          args: {}", app.arguments().join(" "));
        }
    }

    println!(
        "\n{shown} shown of {} found ({packaged} packaged, {} from the Start Menu)",
        apps.len(),
        apps.len() - packaged
    );
}

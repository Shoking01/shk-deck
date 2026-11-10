//! SS550 StreamCraft Controller GUI
//!
//! Run: `cargo run -p ss550-driver --example gui`
//!
//! Light theme, large readable text, and high-contrast panels so every
//! control states exactly what it does.

use ss550_driver::{ButtonImage, Device};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Color {
    #[default]
    White,
    Red,
}

impl Color {
    fn label(&self) -> &'static str {
        match self {
            Color::White => "White",
            Color::Red => "Red",
        }
    }
}

/// Application state
struct AppState {
    device: Option<Device>,
    colors: [Color; 15],
    selected_color: Color,
    error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            device: None,
            colors: [Color::White; 15],
            selected_color: Color::White,
            error: None,
        }
    }
}

impl AppState {
    fn connect(&mut self) {
        if self.device.is_some() {
            return;
        }
        match Device::connect() {
            Ok(mut dev) => {
                if let Err(e) = dev.initialize() {
                    self.error = Some(format!("Init failed: {e}"));
                } else {
                    self.error = None;
                }
                self.device = Some(dev);
            }
            Err(e) => {
                self.error = Some(format!("Connect failed: {e}"));
            }
        }
    }

    fn paint(&mut self, key_idx: usize) {
        if let Some(ref mut dev) = self.device {
            let key_u8 = key_idx as u8;
            let img_result = match self.selected_color {
                Color::White => ButtonImage::white(key_u8),
                Color::Red => ButtonImage::red(key_u8),
            };

            match img_result {
                Ok(image) => {
                    if let Err(e) = dev.set_key_image(image) {
                        self.error = Some(format!("Paint failed: {e}"));
                    }
                    self.colors[key_idx] = self.selected_color;
                    let _ = dev.keepalive();
                }
                Err(e) => {
                    self.error = Some(format!("Image error: {e}"));
                }
            }
        }
    }

    fn clear_all(&mut self) {
        if let Some(ref mut dev) = self.device {
            let _ = dev.clear_all();
            self.colors = [Color::White; 15];
        }
    }
}

struct App {
    state: Mutex<AppState>,
    theme_set: std::sync::atomic::AtomicBool,
}

impl App {
    fn new(state: Mutex<AppState>) -> Self {
        Self {
            state,
            theme_set: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Big, explicit heading text (guaranteed readable on the light theme).
fn big(ui: &mut eframe::egui::Ui, text: &str) {
    ui.add(
        eframe::egui::Label::new(
            eframe::egui::RichText::new(text)
                .size(22.0)
                .strong()
                .color(eframe::egui::Color32::BLACK),
        )
        .wrap(),
    );
}

/// Section body text.
fn body(ui: &mut eframe::egui::Ui, text: &str) {
    ui.add(
        eframe::egui::Label::new(
            eframe::egui::RichText::new(text)
                .size(18.0)
                .color(eframe::egui::Color32::BLACK),
        )
        .wrap(),
    );
}

impl eframe::App for App {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        // Force a light, high-contrast theme once.
        if !self
            .theme_set
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let mut visuals = eframe::egui::Visuals::light();
            visuals.override_text_color = Some(eframe::egui::Color32::BLACK);
            visuals.widgets.inactive.fg_stroke.color = eframe::egui::Color32::BLACK;
            visuals.widgets.hovered.fg_stroke.color = eframe::egui::Color32::BLACK;
            visuals.widgets.active.fg_stroke.color = eframe::egui::Color32::BLACK;
            visuals.panel_fill = eframe::egui::Color32::from_rgb(245, 245, 245);
            ctx.set_visuals(visuals);
        }

        // Connect on first frame.
        {
            let mut state = self.state.lock().unwrap();
            state.connect();
        }

        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                big(ui, "SS550 StreamCraft Controller");
                ui.add_space(6.0);

                // === DEVICE STATUS PANEL (explicit colored frame) ===
                {
                    let state = self.state.lock().unwrap();
                    let (fill, msg) = if let Some(ref err) = state.error {
                        (
                            eframe::egui::Color32::from_rgb(255, 150, 150),
                            format!("DEVICE:  ERROR — {}", err),
                        )
                    } else if state.device.is_some() {
                        (
                            eframe::egui::Color32::from_rgb(150, 255, 150),
                            "DEVICE:  CONNECTED ✓".to_string(),
                        )
                    } else {
                        (
                            eframe::egui::Color32::from_rgb(255, 230, 150),
                            "DEVICE:  CONNECTING...".to_string(),
                        )
                    };

                    eframe::egui::Frame::group(&eframe::egui::Style::default())
                        .fill(fill)
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            body(ui, &msg);
                        });
                }
                ui.add_space(10.0);

                // === PAINT COLOR SELECTOR ===
                big(ui, "PAINT COLOR — click one:");
                ui.horizontal(|ui| {
                    let is_white = self.state.lock().unwrap().selected_color == Color::White;
                    let is_red = self.state.lock().unwrap().selected_color == Color::Red;

                    if ui
                        .add_sized(
                            [140.0, 50.0],
                            eframe::egui::Button::new(
                                eframe::egui::RichText::new(if is_white {
                                    "[✓] WHITE"
                                } else {
                                    "[ ] WHITE"
                                })
                                .size(20.0)
                                .color(eframe::egui::Color32::BLACK),
                            ),
                        )
                        .clicked()
                    {
                        self.state.lock().unwrap().selected_color = Color::White;
                    }
                    ui.add_space(15.0);
                    if ui
                        .add_sized(
                            [140.0, 50.0],
                            eframe::egui::Button::new(
                                eframe::egui::RichText::new(if is_red {
                                    "[✓] RED"
                                } else {
                                    "[ ] RED"
                                })
                                .size(20.0)
                                .color(eframe::egui::Color32::BLACK),
                            ),
                        )
                        .clicked()
                    {
                        self.state.lock().unwrap().selected_color = Color::Red;
                    }
                });
                body(
                    ui,
                    &format!(
                        "Now painting with: {}",
                        self.state.lock().unwrap().selected_color.label()
                    ),
                );
                ui.add_space(10.0);

                // === ACTIONS ===
                big(ui, "ACTIONS — click to run:");
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(
                            [200.0, 60.0],
                            eframe::egui::Button::new(
                                eframe::egui::RichText::new("CLEAR ALL\n(reset to White)")
                                    .size(16.0)
                                    .color(eframe::egui::Color32::BLACK),
                            ),
                        )
                        .clicked()
                    {
                        self.state.lock().unwrap().clear_all();
                    }
                    ui.add_space(15.0);
                    if ui
                        .add_sized(
                            [200.0, 60.0],
                            eframe::egui::Button::new(
                                eframe::egui::RichText::new("HEARTBEAT\n(prevent timeout)")
                                    .size(16.0)
                                    .color(eframe::egui::Color32::BLACK),
                            ),
                        )
                        .clicked()
                    {
                        if let Some(ref mut dev) = self.state.lock().unwrap().device {
                            let _ = dev.keepalive();
                        }
                    }
                });
                ui.add_space(10.0);

                // === LCD BUTTON GRID ===
                big(ui, "15 LCD BUTTONS — click to paint:");
                body(ui, "Each button shows its current color in its own label.");
                ui.add_space(6.0);

                let rows: [(&str, Vec<u8>); 3] = [
                    ("TOP ROW — keys 11-15", vec![11, 12, 13, 14, 15]),
                    ("MIDDLE ROW — keys 6-10", vec![6, 7, 8, 9, 10]),
                    ("BOTTOM ROW — keys 1-5", vec![1, 2, 3, 4, 5]),
                ];
                // Flat array index per display key: top=array 0..5, mid=5..10, bottom=10..15
                let idx_base = |display: u8| -> usize {
                    match display {
                        11..=15 => (display - 11) as usize,
                        6..=10 => (display - 6 + 5) as usize,
                        1..=5 => (display - 1 + 10) as usize,
                        _ => 0,
                    }
                };

                for (label, keys) in rows {
                    big(ui, label);
                    eframe::egui::Grid::new(label)
                        .spacing([6.0, 6.0])
                        .show(ui, |ui| {
                            for key_num in keys {
                                let arr_idx = idx_base(key_num);
                                let current = self.state.lock().unwrap().colors[arr_idx];
                                let txt = format!("#{}\n{}", key_num, current.label());
                                let tile = if current == Color::White {
                                    // Light gray tile = white state, black text = readable.
                                    eframe::egui::Color32::from_rgb(220, 220, 220)
                                } else {
                                    // Red tile = red state, black text = readable.
                                    eframe::egui::Color32::from_rgb(255, 140, 140)
                                };
                                let resp = ui.add_sized(
                                    [105.0, 70.0],
                                    eframe::egui::Button::new(
                                        eframe::egui::RichText::new(txt)
                                            .size(16.0)
                                            .color(eframe::egui::Color32::BLACK),
                                    )
                                    .fill(tile),
                                );
                                if resp.clicked() {
                                    self.state.lock().unwrap().paint(arr_idx);
                                }
                            }
                        });
                    ui.add_space(8.0);
                }

                // Keep the deck alive on every frame.
                if let Some(ref mut dev) = self.state.lock().unwrap().device {
                    let _ = dev.keepalive();
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([900.0, 1100.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SS550 Controller",
        options,
        Box::new(|_cc| {
            let state = Mutex::new(AppState::default());
            Ok(Box::new(App::new(state)))
        }),
    )
}

//! REA RD v2 — native host (first vertical slice).
//!
//! This is the real v2 application skeleton, not the spike: it talks to the
//! official Mirabox SDK server over WebSocket (the same proven transport the
//! Electron host used), reads the user's existing profile, and renders the
//! approved look (egui chassis + hand-painted widget layer).
//!
//! Scope of this slice: transport, profile, UI, real key painting.
//! The HTML plugin runtime (wry) is a later task.

mod device;
mod profile;
mod widgets;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use device::{DeviceEvent, DeviceHandle};
use eframe::egui;
use profile::{COLUMNS, Profile, ROWS, key_index_of};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use widgets::{Btn, DANGER, MUTED, OK, TEXT, WARN, badge, custom_button};

const CELL: f32 = 96.0;
const GAP: f32 = 10.0;

struct DeckApp {
    profile: Profile,
    /// Key previews decoded from the current host dumps (real device pixels).
    key_textures: BTreeMap<usize, egui::TextureHandle>,
    /// Same dumps as base64, ready to send to the device.
    dump_images: BTreeMap<usize, String>,
    selected: Option<usize>,
    device: DeviceHandle,
    sdk_connected: bool,
    device_connected: bool,
    device_label: Option<String>,
    last_error: Option<String>,
    last_key_event: Option<String>,
}

impl DeckApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        widgets::install_theme(&cc.egui_ctx);

        // v2/ -> workspace root is one level up; the current host's data lives in host/.
        let host_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("host");
        let profile = Profile::load(&host_dir.join("profile.json"));

        let mut key_textures = BTreeMap::new();
        let mut dump_images = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&host_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(rest) = name.strip_prefix("last-setImage-key") else { continue };
                let Some(num_part) = rest.strip_suffix(".png") else { continue };
                let Ok(idx) = num_part.parse::<usize>() else { continue };
                let Ok(bytes) = std::fs::read(entry.path()) else { continue };
                let Ok(img) = image::load_from_memory(&bytes) else { continue };
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                key_textures.insert(idx, cc.egui_ctx.load_texture(format!("key-{idx}"), color, egui::TextureOptions::LINEAR));
                dump_images.insert(idx, BASE64.encode(&bytes));
            }
        }

        Self {
            profile,
            key_textures,
            dump_images,
            selected: None,
            device: DeviceHandle::spawn(device::SDK_URL),
            sdk_connected: false,
            device_connected: false,
            device_label: None,
            last_error: None,
            last_key_event: None,
        }
    }

    fn poll_device(&mut self) {
        while let Ok(event) = self.device.events.try_recv() {
            match event {
                DeviceEvent::ServerConnected => {
                    eprintln!("[v2] SDK conectado");
                    self.sdk_connected = true;
                    self.last_error = None;
                }
                DeviceEvent::ServerDisconnected => {
                    eprintln!("[v2] SDK desconectado");
                    self.sdk_connected = false;
                    self.device_connected = false;
                    self.device_label = None;
                }
                DeviceEvent::DeviceConnected { info, .. } => {
                    self.device_connected = true;
                    let serial = info.get("SerialNumber").and_then(|v| v.as_str()).unwrap_or("");
                    let product = info.get("Product").and_then(|v| v.as_str()).unwrap_or("SS550V3");
                    eprintln!("[v2] deck conectada: {product} {serial}");
                    self.device_label = Some(format!("{product} · {serial}"));
                    // Parity with the Electron host: restore the last known deck
                    // state on connect so the device is never left dark.
                    self.paint_all();
                }
                DeviceEvent::DeviceDisconnected => {
                    eprintln!("[v2] deck desconectada");
                    self.device_connected = false;
                    self.device_label = None;
                }
                DeviceEvent::Key(payload) => {
                    self.last_key_event = Some(format!("tecla: {payload}"));
                }
                DeviceEvent::Error(error) => {
                    if self.last_error.is_none() {
                        eprintln!("[v2] SDK error: {error}");
                    }
                    self.sdk_connected = false;
                    self.last_error = Some(error);
                }
            }
        }
    }

    fn status(&self) -> (&'static str, egui::Color32) {
        if self.device_connected {
            ("deck conectada", OK)
        } else if self.sdk_connected {
            ("SDK listo · buscando deck", WARN)
        } else {
            ("SDK desconectado", DANGER)
        }
    }

    fn paint_key(&self, idx: usize) {
        if let Some(data) = self.dump_images.get(&idx) {
            self.device.set_key(idx as u32, data.clone());
        }
    }

    fn paint_all(&self) {
        for (idx, data) in &self.dump_images {
            self.device.set_key(*idx as u32, data.clone());
        }
    }
}

impl eframe::App for DeckApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_device();
        // The device thread is event driven; poll its channel a few times a
        // second so the badge and previews stay honest without busy looping.
        ui.ctx().request_repaint_after(Duration::from_millis(250));

        let (status_text, status_dot) = self.status();

        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.strong("REA RD v2");
                ui.add_space(8.0);
                badge(ui, status_text, status_dot);
                if let Some(label) = &self.device_label {
                    ui.label(egui::RichText::new(label).small().color(MUTED));
                }
            });
            ui.add_space(6.0);
        });

        egui::Panel::right("panel")
            .resizable(false)
            .show(ui, |ui| {
                ui.set_min_width(320.0);
                ui.add_space(10.0);
                match self.selected {
                    Some(idx) => {
                        ui.heading(format!("Tecla {idx}"));
                        ui.add_space(2.0);
                        match self.profile.instance_at(idx) {
                            Some(instance) => {
                                ui.label(egui::RichText::new("Asignada").weak());
                                ui.add_space(8.0);
                                ui.label(egui::RichText::new("Plugin").small().color(MUTED));
                                ui.label(egui::RichText::new(&instance.plugin).monospace().color(TEXT));
                                ui.label(egui::RichText::new("Acción").small().color(MUTED));
                                ui.label(egui::RichText::new(&instance.action).monospace().color(TEXT));
                                if let Some(color) = &instance.title_color {
                                    ui.label(egui::RichText::new("Color del título").small().color(MUTED));
                                    ui.horizontal(|ui| {
                                        let parsed = parse_hex_color(color).unwrap_or(egui::Color32::WHITE);
                                        let (swatch, _) = ui.allocate_exact_size(egui::Vec2::new(16.0, 16.0), egui::Sense::hover());
                                        ui.painter().rect_filled(swatch, egui::CornerRadius::same(4), parsed);
                                        ui.label(egui::RichText::new(color).monospace().color(MUTED));
                                    });
                                }
                            }
                            None => {
                                ui.label(egui::RichText::new("Vacía").weak());
                            }
                        }
                        ui.add_space(14.0);
                        let has_dump = self.dump_images.contains_key(&idx);
                        ui.add_enabled_ui(has_dump, |ui| {
                            if custom_button(ui, "Probar tecla", Btn::Primary) {
                                self.paint_key(idx);
                            }
                        });
                        if !has_dump {
                            ui.label(egui::RichText::new("Sin imagen de esta tecla todavía.").small().color(MUTED));
                        }
                    }
                    None => {
                        ui.heading("Configuración");
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new("Elige una tecla del deck.").weak());
                    }
                }

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(egui::RichText::new("TRANSPORTE").small().extra_letter_spacing(1.2).color(MUTED));
                ui.add_space(4.0);
                if custom_button(ui, "Pintar deck", Btn::Secondary) {
                    self.paint_all();
                }
                ui.add_space(6.0);
                if let Some(event) = &self.last_key_event {
                    ui.label(egui::RichText::new(event).small().color(MUTED));
                }
                if let Some(error) = &self.last_error {
                    ui.label(egui::RichText::new(error).small().color(DANGER));
                }
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("La edición completa del perfil llega en la siguiente tarea.")
                        .small()
                        .color(MUTED),
                );
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(widgets::BG).inner_margin(18.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("TOCA UNA TECLA PARA VER SU ASIGNACIÓN")
                        .small()
                        .extra_letter_spacing(1.2)
                        .color(MUTED),
                );
                ui.add_space(10.0);
                ui.spacing_mut().item_spacing = egui::Vec2::splat(GAP);
                for row in 0..ROWS {
                    ui.horizontal(|ui| {
                        for column in 0..COLUMNS {
                            let idx = key_index_of(row, column);
                            let assigned = self.profile.instance_at(idx).is_some();
                            let texture = self.key_textures.get(&idx);
                            let resp = widgets::deck_cell(ui, idx, self.selected == Some(idx), assigned, texture, CELL);
                            if resp.clicked() {
                                self.selected = Some(idx);
                            }
                            resp.on_hover_text(format!("Tecla {idx}"));
                        }
                    });
                }
            });
    }
}

fn parse_hex_color(value: &str) -> Option<egui::Color32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(egui::Color32::from_rgb(r, g, b))
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("REA RD v2")
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([920.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native("REA RD v2", options, Box::new(|cc| Ok(Box::new(DeckApp::new(cc)))))
}

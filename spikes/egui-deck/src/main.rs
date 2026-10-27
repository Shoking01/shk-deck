// REA RD v2 spike: egui look-and-feel prototype for the deck configurator.
//
// NOT the v2 host. This window exists to answer one question: does a native
// egui UI feel right for REA RD? It renders the 5x3 deck replica with the REAL
// key images the current Electron host dumped while running (host/last-setImage-
// key*.png) and the real profile assignments (host/profile.json), plus a mock
// of the details panel, all in Spanish like the polished config UI.
//
// Everything here is disposable. If the look is approved, the v2 host gets its
// own real project; the styling tokens demonstrated here carry over.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui::{Color32, ColorImage, CornerRadius, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

const COLUMNS: usize = 5;
const ROWS: usize = 3;
const CELL: f32 = 96.0;
const GAP: f32 = 10.0;

// Design tokens mirroring the polished config UI (config.html). One place to
// retune the whole look during the spike.
const BG: Color32 = Color32::from_rgb(0x14, 0x15, 0x1a);
const SURFACE: Color32 = Color32::from_rgb(0x1e, 0x20, 0x27);
const BORDER: Color32 = Color32::from_rgb(0x33, 0x36, 0x3f);
const BORDER_ASSIGNED: Color32 = Color32::from_rgb(0x3d, 0x5a, 0x86);
const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8c, 0xff);
const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe9, 0xec);
const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xad);
const OK: Color32 = Color32::from_rgb(0x3f, 0xbf, 0x6f);

#[derive(Deserialize)]
struct Profile {
    #[serde(default)]
    instances: Vec<Instance>,
}

#[derive(Deserialize)]
struct Instance {
    column: usize,
    row: usize,
    #[serde(default)]
    action: String,
}

struct DeckApp {
    key_textures: BTreeMap<usize, TextureHandle>,
    assigned_keys: HashSet<usize>,
    selected: Option<usize>,
    mock_action: String,
    mock_app: String,
    mock_args: String,
    mock_color: Color32,
}

impl DeckApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        // Spike root: spikes/egui-deck -> workspace root is two levels up.
        let host_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("host");

        // Theme: dark base + our tokens, applied to every theme variant so a
        // system theme switch cannot break the look. Everything the spike
        // shows is retunable from this one block.
        cc.egui_ctx.all_styles_mut(|style| {
            style.visuals.dark_mode = true;
            style.visuals.panel_fill = SURFACE;
            style.visuals.window_fill = SURFACE;
            style.visuals.override_text_color = Some(TEXT);
        });

        let key_textures = load_key_textures(&cc.egui_ctx, &host_dir);
        let assigned_keys = load_assigned_keys(&host_dir);

        Self {
            key_textures,
            assigned_keys,
            selected: None,
            mock_action: "Elige una acción...".to_string(),
            mock_app: String::new(),
            mock_args: String::new(),
            mock_color: Color32::WHITE,
        }
    }
}

// The current host dumps every setImage it forwards as host/last-setImage-keyN.png.
// Those dumps ARE the live pixels the device shows, so the spike renders them.
fn load_key_textures(ctx: &egui::Context, host_dir: &PathBuf) -> BTreeMap<usize, TextureHandle> {
    let mut map = BTreeMap::new();
    let entries = match std::fs::read_dir(host_dir) {
        Ok(e) => e,
        Err(_) => return map,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("last-setImage-key") else { continue };
        let Some(num_part) = rest.strip_suffix(".png") else { continue };
        let Ok(idx) = num_part.parse::<usize>() else { continue };
        let Ok(bytes) = std::fs::read(entry.path()) else { continue };
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let tex = ctx.load_texture(format!("key-{idx}"), color, TextureOptions::LINEAR);
        map.insert(idx, tex);
    }
    map
}

fn load_assigned_keys(host_dir: &PathBuf) -> HashSet<usize> {
    let mut set = HashSet::new();
    let Ok(text) = std::fs::read_to_string(host_dir.join("profile.json")) else { return set };
    if let Ok(profile) = serde_json::from_str::<Profile>(&text) {
        for inst in profile.instances {
            if inst.row < ROWS && inst.column < COLUMNS {
                set.insert(inst.row * COLUMNS + inst.column + 1);
            }
        }
    }
    set
}

impl eframe::App for DeckApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Header: title + device badge.
        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.strong("REA RD v2 — spike egui");
                ui.add_space(8.0);
                badge(ui, "SS550V3 · conectado", OK);
            });
            ui.add_space(6.0);
        });

        // Details panel mock (right side), Spanish like the polished config UI.
        egui::Panel::right("panel")
            .resizable(false)
            .show(ui, |ui| {
                ui.set_min_width(320.0);
                ui.add_space(10.0);
                ui.heading(match self.selected {
                    Some(idx) => format!("Tecla {idx}"),
                    None => "Configuración".to_string(),
                });
                ui.add_space(2.0);
                if let Some(idx) = self.selected {
                    if self.assigned_keys.contains(&idx) {
                        ui.label(egui::RichText::new("Asignada").weak());
                    } else {
                        ui.label(egui::RichText::new("Vacía - elige qué hará esta tecla").weak());
                    }
                } else {
                    ui.label(egui::RichText::new("Elige una tecla del deck para asignarle una acción.").weak());
                }
                ui.add_space(12.0);

                ui.add_enabled_ui(self.selected.is_some(), |ui| {
                    ui.label("Acción");
                    egui::ComboBox::from_id_salt("action-select")
                        .width(240.0)
                        .selected_text(&self.mock_action)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.mock_action, "Elige una acción...".to_string(), "Elige una acción...");
                            ui.separator();
                            ui.selectable_value(&mut self.mock_action, "Abrir aplicación".to_string(), "Abrir aplicación");
                            ui.selectable_value(&mut self.mock_action, "Reloj mundial".to_string(), "Reloj mundial");
                            ui.selectable_value(&mut self.mock_action, "Calendario".to_string(), "Calendario");
                        });

                    if self.mock_action == "Abrir aplicación" {
                        ui.label("Aplicación");
                        ui.add(egui::TextEdit::singleline(&mut self.mock_app).hint_text("Escribe para buscar apps instaladas...").desired_width(240.0));
                        ui.label(egui::RichText::new("Argumentos (opcional)").weak());
                        ui.add(egui::TextEdit::singleline(&mut self.mock_args).desired_width(240.0));
                    }

                    ui.label("Color del título");
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgba(&mut self.mock_color);
                        ui.label(egui::RichText::new(format!("#{:02x}{:02x}{:02x}", self.mock_color.r(), self.mock_color.g(), self.mock_color.b())).monospace());
                    });
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Guardar").strong()).fill(ACCENT)).clicked() {
                        // Spike: no persistence, the flash would go here.
                    }
                    if ui.button("Quitar").clicked() {}
                    if ui.button("Cancelar").clicked() {
                        self.selected = None;
                    }
                });
            });

        // Deck replica.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG).inner_margin(18.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("TOCA UNA TECLA PARA CONFIGURARLA").small().extra_letter_spacing(1.2).color(MUTED));
                ui.add_space(10.0);
                ui.spacing_mut().item_spacing = Vec2::splat(GAP);
                for row in 0..ROWS {
                    ui.horizontal(|ui| {
                        for column in 0..COLUMNS {
                            let idx = row * COLUMNS + column + 1;
                            self.draw_cell(ui, idx);
                        }
                    });
                }
            });
    }
}

impl DeckApp {
    fn draw_cell(&mut self, ui: &mut egui::Ui, idx: usize) {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CELL), Sense::click());
        let resp = resp.on_hover_text(format!("Tecla {idx}"));
        if resp.clicked() {
            self.selected = Some(idx);
        }
        let selected = self.selected == Some(idx);
        let assigned = self.assigned_keys.contains(&idx);

        let painter = ui.painter_at(rect);
        // Backing tile: dark surface, or the real key pixels when we have them.
        painter.rect_filled(rect, CornerRadius::same(10), SURFACE);
        if let Some(tex) = self.key_textures.get(&idx) {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let stroke = if selected {
            Stroke::new(2.0, ACCENT)
        } else if assigned {
            Stroke::new(1.0, BORDER_ASSIGNED)
        } else {
            Stroke::new(1.0, BORDER)
        };
        painter.rect_stroke(rect, CornerRadius::same(10), stroke, egui::StrokeKind::Inside);
        if selected {
            // Selection halo like the config UI's box-shadow.
            let halo = rect.expand(2.0);
            let halo_color = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 140);
            ui.painter_at(halo).rect_stroke(halo, CornerRadius::same(12), Stroke::new(2.0, halo_color), egui::StrokeKind::Inside);
        }
        if !assigned && self.key_textures.get(&idx).is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "+",
                egui::FontId::proportional(20.0),
                MUTED,
            );
        }
    }
}

fn badge(ui: &mut egui::Ui, text: &str, dot: Color32) {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(11))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 4.0, dot);
                ui.label(egui::RichText::new(text).small().color(MUTED));
            });
        });
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("REA RD v2 — spike egui")
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([920.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "REA RD v2 spike",
        options,
        Box::new(|cc| Ok(Box::new(DeckApp::new(cc)))),
    )
}

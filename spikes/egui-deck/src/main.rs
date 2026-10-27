// REA RD v2 spike: egui look-and-feel prototype for the deck configurator.
//
// NOT the v2 host. This window exists to answer one question: does a native
// egui UI feel right for REA RD? It renders the 5x3 deck replica with the REAL
// key images the current Electron host dumped while running (host/last-setImage-
// key*.png) and the real profile assignments (host/profile.json), plus a mock
// of the details panel, all in Spanish like the polished config UI.
//
// v2 of the spike: egui is only the CHASSIS (layout, input, text, popups).
// Every visible control is hand-painted here: buttons with animated hover and
// press states, a custom dropdown, custom-framed text fields, deck cells with
// hover transitions. No default egui widget look remains on screen.
//
// Everything here is disposable. If the look is approved, the v2 host gets its
// own real project; the widget layer demonstrated here carries over.

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
const SURFACE_2: Color32 = Color32::from_rgb(0x26, 0x29, 0x31);
const BORDER: Color32 = Color32::from_rgb(0x33, 0x36, 0x3f);
const BORDER_ASSIGNED: Color32 = Color32::from_rgb(0x3d, 0x5a, 0x86);
const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8c, 0xff);
const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe9, 0xec);
const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xad);
const OK: Color32 = Color32::from_rgb(0x3f, 0xbf, 0x6f);
const DANGER: Color32 = Color32::from_rgb(0xe5, 0x6c, 0x5c);
const COMBO_PLACEHOLDER: &str = "Elige una acción...";

fn danger_bg() -> Color32 {
    Color32::from_rgba_unmultiplied(0xe5, 0x6c, 0x5c, 26)
}

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
        // system theme switch cannot break the look. The widgets we do NOT
        // paint ourselves (the color picker) get restyled here too.
        cc.egui_ctx.all_styles_mut(|style| {
            style.visuals.dark_mode = true;
            style.visuals.panel_fill = SURFACE;
            style.visuals.window_fill = SURFACE;
            style.visuals.override_text_color = Some(TEXT);
            style.visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 70);
            style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
            style.visuals.widgets.inactive.bg_fill = SURFACE_2;
            style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
            style.visuals.widgets.hovered.bg_fill = mix(SURFACE_2, TEXT, 0.08);
            style.visuals.widgets.active.bg_fill = mix(SURFACE_2, BG, 0.3);
        });

        let key_textures = load_key_textures(&cc.egui_ctx, &host_dir);
        let assigned_keys = load_assigned_keys(&host_dir);

        Self {
            key_textures,
            assigned_keys,
            selected: None,
            mock_action: COMBO_PLACEHOLDER.to_string(),
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
                    if let Some(choice) = combo_field(
                        ui,
                        "action",
                        &self.mock_action,
                        &[COMBO_PLACEHOLDER, "Abrir aplicación", "Reloj mundial", "Calendario"],
                    ) {
                        self.mock_action = choice;
                    }

                    if self.mock_action == "Abrir aplicación" {
                        ui.label("Aplicación");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.mock_app)
                                .hint_text("Escribe para buscar apps instaladas...")
                                .desired_width(240.0)
                                .frame(
                                    egui::Frame::new()
                                        .fill(SURFACE_2)
                                        .stroke(Stroke::new(1.0, BORDER))
                                        .corner_radius(CornerRadius::same(8))
                                        .inner_margin(egui::Margin::symmetric(10, 6)),
                                ),
                        );
                        ui.label(egui::RichText::new("Argumentos (opcional)").weak());
                        ui.add(
                            egui::TextEdit::singleline(&mut self.mock_args)
                                .desired_width(240.0)
                                .frame(
                                    egui::Frame::new()
                                        .fill(SURFACE_2)
                                        .stroke(Stroke::new(1.0, BORDER))
                                        .corner_radius(CornerRadius::same(8))
                                        .inner_margin(egui::Margin::symmetric(10, 6)),
                                ),
                        );
                    }

                    ui.label("Color del título");
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgba(&mut self.mock_color);
                        ui.label(egui::RichText::new(format!("#{:#02x}{:#02x}{:#02x}", self.mock_color.r(), self.mock_color.g(), self.mock_color.b())).monospace());
                    });
                });

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if custom_button(ui, "Guardar", Btn::Primary) {
                        // Spike: no persistence, the flash would go here.
                    }
                    if custom_button(ui, "Quitar", Btn::Danger) {}
                    if custom_button(ui, "Cancelar", Btn::Ghost) {
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
        let t = ui.ctx().animate_bool_with_time(ui.id().with(("cell", idx)), resp.hovered(), 0.08);

        let painter = ui.painter_at(rect);
        // Backing tile: dark surface that brightens on hover, or the real key
        // pixels when we have them.
        painter.rect_filled(rect, CornerRadius::same(10), mix(SURFACE, mix(SURFACE, TEXT, 0.06), t));
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
            Stroke::new(1.0, mix(BORDER, TEXT, t * 0.5))
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

// ---------- Hand-painted widget layer (the v2 spike point) ----------

#[derive(Clone, Copy)]
enum Btn {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// A fully hand-painted button: no egui Button widget involved. Animated
/// hover (0.08s) and press (0.06s) transitions via ctx.animate_bool_with_time.
fn custom_button(ui: &mut egui::Ui, label: &str, variant: Btn) -> bool {
    let font = egui::FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), TEXT);
    let size = Vec2::new(galley.rect.width() + 32.0, galley.rect.height() + 16.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let id = ui.id().with(("btn", label));
    let t_h = ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.08);
    let t_p = ui.ctx().animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.06);

    let (fill_a, fill_b, text_col, stroke_col) = match variant {
        Btn::Primary => (ACCENT, mix(ACCENT, Color32::WHITE, 0.15), Color32::WHITE, ACCENT),
        Btn::Secondary => (SURFACE_2, mix(SURFACE_2, TEXT, 0.08), TEXT, BORDER),
        Btn::Ghost => (Color32::TRANSPARENT, SURFACE_2, MUTED, Color32::TRANSPARENT),
        Btn::Danger => (Color32::TRANSPARENT, danger_bg(), DANGER, DANGER),
    };
    let fill = mix(fill_a.lerp_to_gamma(fill_b, t_h), BG, t_p * 0.4);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(8), fill);
    if stroke_col != Color32::TRANSPARENT {
        painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, stroke_col), egui::StrokeKind::Inside);
    }
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, font, text_col);
    resp.clicked()
}

/// A fully hand-painted dropdown field: the closed field is our own rect, and
/// the option list opens in an egui Popup whose rows are also hand-painted.
/// Returns Some(choice) when the user picks an option.
fn combo_field(ui: &mut egui::Ui, id_salt: &str, selected: &str, options: &[&str]) -> Option<String> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(240.0, 30.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(ui.id().with((id_salt, "hover")), resp.hovered(), 0.08);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(8), SURFACE_2);
    painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, mix(BORDER, ACCENT, t)), egui::StrokeKind::Inside);
    painter.text(
        Pos2::new(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        selected,
        egui::FontId::proportional(14.0),
        if selected == COMBO_PLACEHOLDER { MUTED } else { TEXT },
    );
    painter.text(
        Pos2::new(rect.right() - 12.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "▾",
        egui::FontId::proportional(12.0),
        MUTED,
    );

    let mut chosen = None;
    egui::Popup::menu(&resp)
        .width(240.0)
        .gap(4.0)
        .show(|ui| {
            for opt in options {
                let (r, rr) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
                let ht = ui.ctx().animate_bool_with_time(ui.id().with(("opt", opt)), rr.hovered(), 0.06);
                let p = ui.painter_at(r);
                if *opt == selected {
                    p.rect_filled(r, CornerRadius::same(6), Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 38));
                } else if rr.hovered() {
                    p.rect_filled(r, CornerRadius::same(6), mix(SURFACE, TEXT, ht * 0.08));
                }
                p.text(
                    Pos2::new(r.left() + 10.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    *opt,
                    egui::FontId::proportional(13.0),
                    if *opt == selected { ACCENT } else { TEXT },
                );
                if rr.clicked() {
                    chosen = Some((*opt).to_string());
                    ui.close();
                }
            }
        });
    chosen
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

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
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

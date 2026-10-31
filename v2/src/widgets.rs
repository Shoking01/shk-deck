//! Hand-painted widget layer — the approved spike look, now the seed of v2's
//! widget library.
//!
//! egui is only the chassis here: layout, input, text and popups. Every
//! visible control is painted in this module, so the app's look lives in one
//! place and never depends on egui's default widget styling.

use eframe::egui;
use egui::{Color32, CornerRadius, Pos2, Sense, Stroke, Vec2};

// Design tokens (from the approved spike). Retune the whole app from here.
pub const BG: Color32 = Color32::from_rgb(0x14, 0x15, 0x1a);
pub const SURFACE: Color32 = Color32::from_rgb(0x1e, 0x20, 0x27);
pub const SURFACE_2: Color32 = Color32::from_rgb(0x26, 0x29, 0x31);
pub const BORDER: Color32 = Color32::from_rgb(0x33, 0x36, 0x3f);
pub const BORDER_ASSIGNED: Color32 = Color32::from_rgb(0x3d, 0x5a, 0x86);
pub const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8c, 0xff);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe9, 0xec);
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa0, 0xad);
pub const OK: Color32 = Color32::from_rgb(0x3f, 0xbf, 0x6f);
pub const WARN: Color32 = Color32::from_rgb(0xe0, 0xb0, 0x4c);
pub const DANGER: Color32 = Color32::from_rgb(0xe5, 0x6c, 0x5c);

// Widget-library surface for v2: variants stay available even when a screen
// does not use all of them yet.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum Btn {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// A fully hand-painted button: no egui Button widget involved. Animated
/// hover (80ms) and press (60ms) transitions.
pub fn custom_button(ui: &mut egui::Ui, label: &str, variant: Btn) -> bool {
    let font = egui::FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), TEXT);
    let size = Vec2::new(galley.rect.width() + 32.0, galley.rect.height() + 16.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let id = ui.scope_id().with(("btn", label));
    let t_h = ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.08);
    let t_p = ui.ctx().animate_bool_with_time(id.with("p"), resp.is_pointer_button_down_on(), 0.06);

    let (fill_a, fill_b, text_col, stroke_col) = match variant {
        Btn::Primary => (ACCENT, mix(ACCENT, Color32::WHITE, 0.15), Color32::WHITE, ACCENT),
        Btn::Secondary => (SURFACE_2, mix(SURFACE_2, TEXT, 0.08), TEXT, BORDER),
        Btn::Ghost => (Color32::TRANSPARENT, SURFACE_2, MUTED, Color32::TRANSPARENT),
        Btn::Danger => (
            Color32::TRANSPARENT,
            Color32::from_rgba_unmultiplied(DANGER.r(), DANGER.g(), DANGER.b(), 26),
            DANGER,
            DANGER,
        ),
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

/// Status pill: colored dot + label, painted in one rect.
pub fn badge(ui: &mut egui::Ui, text: &str, dot: Color32) {
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

/// Linear RGB mix, used by every animated state above.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
}

/// Install the app theme: dark base + tokens, applied to every theme variant so
/// a system theme switch cannot break the look.
pub fn install_theme(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
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
}

/// One deck cell: backing tile, real pixels when available, state stroke and
/// animated hover. Returns the click response so the caller owns selection.
pub fn deck_cell(
    ui: &mut egui::Ui,
    idx: usize,
    selected: bool,
    assigned: bool,
    texture: Option<&egui::TextureHandle>,
    size: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let t = ui.ctx().animate_bool_with_time(ui.scope_id().with(("cell", idx)), resp.hovered(), 0.08);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), mix(SURFACE, mix(SURFACE, TEXT, 0.06), t));
    if let Some(tex) = texture {
        painter.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
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
        let halo = rect.expand(2.0);
        let halo_color = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 140);
        ui.painter_at(halo)
            .rect_stroke(halo, CornerRadius::same(12), Stroke::new(2.0, halo_color), egui::StrokeKind::Inside);
    }
    if !assigned && texture.is_none() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "+",
            egui::FontId::proportional(20.0),
            MUTED,
        );
    }
    resp
}

//! The `Stats` page: what each measurement is doing, and what it has been doing.
//!
//! One card per cell of the catalogue: the number, the source it came from, and a
//! sparkline of the last four minutes. The colours are the only judgement here —
//! a card goes amber past a warning line and red past a limit — and the rules for
//! that live in [`crate::history`], where they can be tested without a window.
//!
//! Nothing on this page samples anything: the sampler thread measures, the window
//! paints what it left behind. A page that drew a graph by asking the machine
//! would be the one thing this whole feature set out to avoid.

use gpui_kit::{div, px, AnyElement, Div, IntoElement, ParentElement as _, Styled as _};

use super::{card, muted, one_line, Tokens, FONT_CAPTION, FONT_MICRO, FONT_SUB, RADIUS_SM};
use crate::history::{self, Severity};
use crate::{Controller, Link};

/// How tall one sparkline is.
const PLOT_H: f32 = 56.;

/// How wide one bar of a sparkline is, and the gap after it.
const BAR_W: f32 = 2.;
const BAR_GAP: f32 = 1.;

/// The colour a card's number and plot use at a given severity.
fn tone(severity: Severity, tokens: Tokens) -> gpui_kit::Hsla {
    match severity {
        Severity::Quiet => tokens.accent,
        Severity::Warn => tokens.warn,
        Severity::Alarm => tokens.bad,
    }
}

/// The word a severity adds under a card, and nothing at all for a quiet one.
fn verdict(severity: Severity) -> Option<&'static str> {
    match severity {
        Severity::Quiet => None,
        Severity::Warn => Some("Warm"),
        Severity::Alarm => Some("At the limit"),
    }
}

/// The page.
pub(crate) fn stats_page(
    controller: &mut Controller,
    tokens: Tokens,
    _cx: &mut gpui_kit::Context<Controller>,
) -> Div {
    let mut grid = div().flex().flex_row().flex_wrap().gap(px(12.)).w_full();

    if controller.session.is_none() {
        return body(tokens).child(card(tokens).child(one_line(
            "The stats come from the deck's own sampler. Plug a deck in and they start.",
            FONT_SUB,
            tokens.soft,
        )));
    }

    let cells: Vec<ss550_plugin_api::CellSpec> = controller.plugins.cells().to_vec();
    for spec in cells {
        grid = grid.child(metric_card(controller, &spec, tokens));
    }

    let mut page = body(tokens).child(grid);

    // The sampler's health, said once: a frozen number must not pass for a live
    // one, and this is the page a person opens to decide exactly that.
    if let Some(gone) = &controller.sampler_gone {
        page = page.child(one_line(gone.clone(), FONT_CAPTION, tokens.bad));
    }
    if let Some(gone) = &controller.faces_gone {
        page = page.child(one_line(gone.clone(), FONT_CAPTION, tokens.bad));
    }
    page.child(muted(
        "Four minutes of samples, one every two seconds. A card turns amber past a warning line \
         and red at the limit.",
        tokens,
    ))
}

/// The page's own column.
fn body(tokens: Tokens) -> Div {
    let _ = tokens;
    div().flex().flex_col().gap(px(12.)).w_full()
}

/// One cell: its number, its source and its sparkline.
fn metric_card(
    controller: &Controller,
    spec: &ss550_plugin_api::CellSpec,
    tokens: Tokens,
) -> AnyElement {
    let plugin = spec.plugin().clone();
    let cell = spec.cell().clone();
    let reading = controller.samples.get(&plugin, &cell).cloned();
    let series = controller.history.series(&plugin, &cell);
    let severity = reading.as_ref().map_or(Severity::Quiet, history::severity);
    let ink = tone(severity, tokens);

    let value = reading
        .as_ref()
        .map_or_else(|| "-".to_string(), |reading| reading.text().to_string());
    let caption = reading.as_ref().and_then(|reading| {
        reading
            .detail()
            .or_else(|| reading.reason())
            .map(str::to_string)
    });

    let mut card_body = card(tokens)
        .flex()
        .flex_col()
        .gap(px(8.))
        .w(px(232.))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(FONT_MICRO))
                        .text_color(tokens.soft)
                        .child(spec.label().to_string()),
                )
                .child(div().w(px(8.)).h(px(8.)).rounded(px(4.)).bg(gpui_kit::rgb(
                    (u32::from(spec.accent()[0]) << 16)
                        | (u32::from(spec.accent()[1]) << 8)
                        | u32::from(spec.accent()[2]),
                ))),
        )
        .child(div().text_size(px(30.)).text_color(ink).child(value));

    if let Some(caption) = caption {
        card_body = card_body.child(
            div()
                .text_size(px(FONT_MICRO))
                .text_color(tokens.soft)
                .child(caption),
        );
    }

    let is_share = reading
        .as_ref()
        .is_some_and(|reading| reading.fraction().is_some());
    card_body = card_body.child(sparkline(series, is_share, ink, tokens));

    if let Some(word) = verdict(severity) {
        card_body = card_body.child(
            div()
                .text_size(px(FONT_CAPTION))
                .text_color(ink)
                .child(word),
        );
    }
    if !controller.link.eq(&Link::Connected) {
        card_body = card_body.child(one_line(
            "The deck is not connected: this is the last sample it took.",
            FONT_CAPTION,
            tokens.soft,
        ));
    }

    card_body.into_any_element()
}

/// The last few minutes of one cell, as bars.
///
/// Bars rather than a line: a bar is a `div` with a height, which costs the
/// renderer nothing and needs no drawing API, and at two-second samples a finger
/// of bars reads a trend as well as a line does.
fn sparkline(series: &[f32], is_share: bool, ink: gpui_kit::Hsla, tokens: Tokens) -> Div {
    // A share is always drawn 0–100: a processor moving between 41 and 43 should
    // look calm, not like a crisis. Everything else is drawn against its own
    // range, which is what makes a temperature's shape visible at all.
    let (low, high) = if is_share {
        (0.0_f32, 100.0_f32)
    } else {
        history::span_of(series)
    };

    let mut bars = div()
        .flex()
        .flex_row()
        .items_end()
        .gap(px(BAR_GAP))
        .h(px(PLOT_H))
        .w_full()
        .rounded(px(RADIUS_SM))
        .overflow_hidden();

    if series.is_empty() {
        return bars.child(
            div()
                .text_size(px(FONT_MICRO))
                .text_color(tokens.soft)
                .child("waiting for a sample"),
        );
    }

    for value in series {
        let share = ((value - low) / (high - low)).clamp(0.0, 1.0);
        let height = (share * PLOT_H).max(1.0);
        bars = bars.child(
            div()
                .flex_none()
                .w(px(BAR_W))
                .h(px(height))
                .bg(ink.opacity(0.85)),
        );
    }
    bars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_card_says_nothing_and_a_hot_one_says_why() {
        assert_eq!(verdict(Severity::Quiet), None);
        assert_eq!(verdict(Severity::Warn), Some("Warm"));
        assert_eq!(verdict(Severity::Alarm), Some("At the limit"));
    }
}

//! The window's shared visual language: tokens, spacing, type scale, and the
//! few primitives every page is built from.
//!
//! One place owns the palette, so four pages cannot drift apart. The dark
//! values are the design's fixed palette; the light values are derived from the
//! active theme, because a window painted with a dark palette in light mode is
//! an unreadable window. The two colours a key can show are *data* — they are
//! painted by a key tile and never used for a border, a label or a button.

use gpui_kit::component::theme::Theme;
use gpui_kit::{
    div, px, rgb, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window,
};

pub(crate) mod deck;
pub(crate) mod page;
pub(crate) mod palette;
pub(crate) mod panels;
pub(crate) mod shell;

pub(crate) use page::Page;

/// Height of the app bar. The window keeps its native title bar, so this is the
/// first row of the window's own chrome.
pub(crate) const APP_BAR_H: f32 = 56.;

/// Width of the navigation rail.
pub(crate) const RAIL_W: f32 = 216.;

/// Height of the status bar along the bottom.
pub(crate) const STATUS_BAR_H: f32 = 32.;

/// Padding of the scrolling content area.
pub(crate) const PAGE_PAD: f32 = 24.;

/// Radius of a control: a chip, a row, a button-sized box.
pub(crate) const RADIUS_SM: f32 = 8.;

/// Radius of a card.
pub(crate) const RADIUS_MD: f32 = 12.;

/// Radius that rounds a box as far as its own size allows: pills and dots.
pub(crate) const RADIUS_PILL: f32 = 999.;

/// The window's smallest allowed width.
///
/// The shell's own floor: below this the rail, a card of tiles and the column
/// beside it stop fitting, and a desktop window that cuts its own content is
/// worse than one that refuses to be dragged narrower.
pub(crate) const WINDOW_MIN_W: f32 = 1040.;

/// The window's smallest allowed height.
pub(crate) const WINDOW_MIN_H: f32 = 680.;

/// Padding inside a card. Shared with [`card`], and with the arithmetic that
/// works out how wide a grid of tiles has to be.
pub(crate) const CARD_PAD: f32 = 20.;

/// Page heading.
pub(crate) const FONT_TITLE: f32 = 20.;

/// The window's own name in the app bar.
pub(crate) const FONT_APP: f32 = 15.;

/// Body text and control labels.
pub(crate) const FONT_SUB: f32 = 13.;

/// The line under the app bar's name, and card captions.
pub(crate) const FONT_CAPTION: f32 = 12.;

/// Section micro-labels: uppercase, the smallest type in the window.
pub(crate) const FONT_MICRO: f32 = 11.;

/// Hex colour as GPUI `Hsla`.
///
/// The palette is stated in hex, the way a design system states it, and turned
/// into the renderer's colour here instead of at every call site.
pub(crate) fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Every colour the chrome is allowed to use.
///
/// Held as one value and passed by copy, so a page cannot reach for a hex of
/// its own: adding a colour means adding a field here.
#[derive(Clone, Copy)]
pub(crate) struct Tokens {
    /// The window's background.
    pub(crate) bg: Hsla,
    /// A card, and the app bar.
    pub(crate) surface: Hsla,
    /// A raised box inside a card: a placeholder, a swatch chip's bed.
    pub(crate) elevated: Hsla,
    /// A hairline border, and the divider between two areas.
    pub(crate) border: Hsla,
    /// Primary text.
    pub(crate) ink: Hsla,
    /// Secondary text: captions, hints, the status line.
    pub(crate) soft: Hsla,
    /// The one accent: rings, fills, the active rail row.
    pub(crate) accent: Hsla,
    /// The accent as text. A tone away from [`Tokens::accent`] on purpose: the
    /// accent itself does not reach AA contrast against either background.
    pub(crate) accent_ink: Hsla,
    /// The accent at low alpha, for the active row's bed.
    pub(crate) accent_tint: Hsla,
    /// Hover bed for a clickable row or chip.
    pub(crate) hover: Hsla,
    /// Connected.
    pub(crate) good: Hsla,
    /// Waiting, or connecting.
    pub(crate) warn: Hsla,
    /// Lost.
    pub(crate) bad: Hsla,
}

impl Tokens {
    /// The palette for the theme in force.
    pub(crate) fn new(theme: &Theme) -> Self {
        if theme.mode.is_dark() {
            Self::dark()
        } else {
            Self::light(theme)
        }
    }

    /// The design's own palette.
    fn dark() -> Self {
        Self {
            bg: color(0x0b0b0f),
            surface: color(0x141419),
            elevated: color(0x1b1b22),
            border: color(0x26262f),
            ink: color(0xededf0),
            soft: color(0xa1a1ad),
            accent: color(0x7c6cf0),
            accent_ink: color(0xa99bff),
            accent_tint: color(0x7c6cf0).opacity(0.16),
            hover: color(0x1f1f28),
            good: color(0x22c55e),
            warn: color(0xf59e0b),
            bad: color(0xef4444),
        }
    }

    /// The same structure on the light theme's own colours.
    ///
    /// The accent stays the accent; only the text tone steps darker, so it
    /// still reads as the same brand on white and still clears AA.
    fn light(theme: &Theme) -> Self {
        Self {
            bg: theme.background,
            surface: theme.sidebar,
            elevated: theme.group_box,
            border: theme.border,
            ink: theme.foreground,
            soft: theme.muted_foreground,
            accent: color(0x7c6cf0),
            accent_ink: color(0x5b4bd6),
            accent_tint: color(0x7c6cf0).opacity(0.12),
            hover: theme.muted,
            good: color(0x15803d),
            warn: color(0xb45309),
            bad: color(0xb91c1c),
        }
    }
}

/// A card: the surface every page's content sits on.
pub(crate) fn card(tokens: Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .p(px(CARD_PAD))
        .rounded(px(RADIUS_MD))
        .bg(tokens.surface)
        .border_1()
        .border_color(tokens.border)
}

/// A card title: 11 px uppercase in the secondary ink.
pub(crate) fn micro_label(text: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .text_size(px(FONT_MICRO))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.soft)
        .child(text.into())
}

/// Body text in the primary ink.
pub(crate) fn body(text: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .text_size(px(FONT_SUB))
        .text_color(tokens.ink)
        .child(text.into())
}

/// Secondary text: a hint, a caption, the status line.
pub(crate) fn muted(text: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .text_size(px(FONT_SUB))
        .text_color(tokens.soft)
        .child(text.into())
}

/// One line of text that cannot widen its container.
///
/// The text is cut with an ellipsis rather than allowed to push the layout: a
/// picture name, a program path or a URL is the user's text, and the window
/// must stay the size it is whatever they typed.
pub(crate) fn one_line(text: impl Into<SharedString>, size: f32, ink: Hsla) -> Div {
    div()
        .min_w(px(0.))
        .truncate()
        .text_size(px(size))
        .text_color(ink)
        .child(text.into())
}

/// Clip a string to `max_chars` characters, ending it with an ellipsis.
///
/// Counted in characters, not bytes, so a name in any script is cut on a
/// boundary. This is for text that has no layout to clip it — a label inside a
/// control that sizes itself to its text.
pub(crate) fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{kept}…")
}

/// Makes a hand-drawn row behave like a button.
///
/// A row painted from a `div` is not focusable and does not answer the keyboard
/// on its own. This gives it a place in the Tab order, a click, and Enter or
/// Space, so every control the window draws by hand is still reachable without
/// a mouse.
pub(crate) fn activate<C, F>(
    element: Stateful<Div>,
    cx: &mut Context<C>,
    handler: F,
) -> Stateful<Div>
where
    C: Render + 'static,
    F: Fn(&mut C, &mut Window, &mut Context<C>) + Clone + 'static,
{
    let entity = cx.entity();
    let clicked = handler.clone();
    let typed = handler;
    element
        .tab_index(0)
        .cursor_pointer()
        .on_click(
            cx.listener(move |this: &mut C, _: &ClickEvent, window, cx| {
                clicked(this, window, cx);
                cx.notify();
            }),
        )
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                // The row owns no state of its own: it asks its entity to do the
                // same thing the click does.
                entity.update(cx, |this, cx| {
                    typed(this, window, cx);
                    cx.notify();
                });
            }
        })
}

/// A dot that says a state in colour, with its meaning carried by text nearby.
pub(crate) fn dot(tokens: Tokens, fill: Hsla, size: f32) -> Div {
    div()
        .flex_none()
        .w(px(size))
        .h(px(size))
        .rounded(px(RADIUS_PILL))
        .bg(fill)
        .border_1()
        .border_color(tokens.border)
}

/// One option of a small exclusive choice, drawn as a chip.
///
/// The active option is ringed in the accent and tinted, so a two- or
/// four-option choice reads in one glance without a legend.
pub(crate) fn segment<C, F>(
    id: SharedString,
    label: impl Into<SharedString>,
    active: bool,
    tokens: Tokens,
    cx: &mut Context<C>,
    handler: F,
) -> AnyElement
where
    C: Render + 'static,
    F: Fn(&mut C, &mut Window, &mut Context<C>) + Clone + 'static,
{
    let chip = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(30.))
        .px(px(12.))
        .rounded(px(RADIUS_SM))
        // Always two pixels: the ring must not resize the chip when it moves.
        .border_2()
        .border_color(if active { tokens.accent } else { tokens.border })
        .bg(if active {
            tokens.accent_tint
        } else {
            tokens.elevated
        })
        .text_size(px(FONT_MICRO))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if active {
            tokens.accent_ink
        } else {
            tokens.soft
        })
        .child(label.into())
        .hover(|style| style.bg(tokens.hover));

    activate(chip, cx, handler).into_any_element()
}

/// A file path, in the monospace face and cut from the left.
///
/// A path is read by its end — the file name — so the cut is at the start. The
/// parent of this row must carry `min_w(px(0.))`, or nothing would be cut.
pub(crate) fn path_line(
    text: impl Into<SharedString>,
    tokens: Tokens,
    family: &SharedString,
) -> Div {
    div()
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_start()
        .text_size(px(FONT_MICRO))
        .text_color(tokens.soft)
        .font_family(family.clone())
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use super::{shorten, Tokens, PAGE_PAD, RADIUS_MD, RADIUS_PILL, RADIUS_SM};
    use gpui_kit::component::{Theme, ThemeMode};

    #[test]
    fn a_short_string_is_left_alone() {
        assert_eq!(shorten("Deck", 8), "Deck");
        assert_eq!(shorten("", 4), "");
    }

    #[test]
    fn a_long_string_is_cut_at_a_character_boundary() {
        assert_eq!(shorten("012-SOFTWARE_brightness", 11), "012-SOFTWA…");
        // Multi-byte characters count as one each, so the cut never splits one.
        assert_eq!(shorten("áéíóú", 3), "áé…");
    }

    #[test]
    fn the_two_modes_do_not_share_a_background() {
        let mut dark_theme = Theme::default();
        dark_theme.mode = ThemeMode::Dark;
        let mut light_theme = Theme::default();
        light_theme.mode = ThemeMode::Light;

        let dark = Tokens::new(&dark_theme);
        let light = Tokens::new(&light_theme);

        assert_ne!(
            dark.bg, light.bg,
            "light mode must not paint the dark palette"
        );
        assert_ne!(dark.ink, light.ink, "text must follow the theme as well");
        assert_eq!(
            dark.bg,
            super::color(0x0b0b0f),
            "the dark palette is the design's own, not the theme's default"
        );
    }

    #[test]
    fn spacing_is_a_multiple_of_four_and_radii_are_the_three_steps() {
        assert_eq!(PAGE_PAD % 4., 0.);
        assert_eq!(RADIUS_SM, 8.);
        assert_eq!(RADIUS_MD, 12.);
        assert_eq!(RADIUS_PILL, 999.);
    }
}

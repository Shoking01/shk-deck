//! The window's shared visual language: tokens, spacing, type scale, and the
//! few primitives every page is built from.
//!
//! One place owns the palette, so three pages cannot drift apart — and it owns
//! no colour of its own. Every token is read out of the active theme, so the
//! flavour the user chose reaches this crate's chrome and the controls it hosts
//! from the library in the same pass. A hex written here would be a second
//! palette, and the two would drift the first time a flavour was added. The two
//! colours a key can show are *data* — they are painted by a key tile and never
//! used for a border, a label or a button.

use gpui_kit::assets::IconName;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::Colorize as _;
use gpui_kit::component::Icon;
use gpui_kit::{
    div, px, rgb, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window,
};

pub(crate) mod deck;
pub(crate) mod editor;
pub(crate) mod page;
pub(crate) mod pages;
pub(crate) mod palette;
pub(crate) mod panels;
pub(crate) mod shell;
pub(crate) mod stats;
pub(crate) mod titlecontrols;

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
    ///
    /// Every value comes from the theme, including the accent: a flavour states
    /// its own, and the two tones this crate derives from it — the accent as
    /// text, and the accent as a tint — are computed here rather than stored in
    /// each theme file, so a new flavour cannot forget them.
    pub(crate) fn new(theme: &Theme) -> Self {
        let accent = theme.primary;
        let dark = theme.is_dark();

        Self {
            bg: theme.background,
            surface: theme.sidebar,
            elevated: theme.group_box,
            border: theme.border,
            ink: theme.foreground,
            soft: theme.muted_foreground,
            accent,
            // The accent itself is a fill: on a dark bed it reads as a shape, not
            // as a letter. Text in it steps towards the light on a dark theme and
            // towards the dark on a light one, which is the direction that keeps
            // AA contrast without a hand-picked hex per flavour.
            accent_ink: match dark {
                true => accent.lighten(0.14),
                false => accent.darken(0.18),
            },
            accent_tint: accent.opacity(match dark {
                true => 0.16,
                false => 0.12,
            }),
            hover: theme.muted,
            good: theme.success,
            warn: theme.warning,
            bad: theme.danger,
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

/// A bundled icon at the size the window draws it.
///
/// The colour is always stated: a GPUI icon paints in the window's text colour
/// unless it is told otherwise, so an icon that inherited its parent's colour
/// would come out near-white on a card that asked for the secondary ink.
pub(crate) fn icon(glyph: IconName, size: f32, ink: Hsla) -> Icon {
    Icon::new(glyph).size(px(size)).text_color(ink)
}

/// A section label: an icon, then the same micro-caps words.
///
/// The icon never replaces the words. It is a landmark for the eye — the same
/// way a heading rule is — and a reader who cannot see it still has the label
/// that names the section.
pub(crate) fn icon_label(glyph: IconName, text: impl Into<SharedString>, tokens: Tokens) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .text_size(px(FONT_MICRO))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.soft)
        .child(icon(glyph, FONT_MICRO + 2., tokens.soft))
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

/// A flavour's accent, as a filled circle with a hairline round it.
///
/// The hairline is what keeps a pale accent from dissolving into a light card
/// and a dark one from dissolving into a dark one: the circle is a swatch, and a
/// swatch has an edge.
pub(crate) fn swatch(tokens: Tokens, accent: u32, size: f32) -> Div {
    div()
        .flex_none()
        .w(px(size))
        .h(px(size))
        .rounded(px(RADIUS_PILL))
        .bg(color(accent))
        .border_1()
        .border_color(tokens.ink.opacity(0.18))
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
    use super::{color, shorten, Tokens, PAGE_PAD, RADIUS_MD, RADIUS_PILL, RADIUS_SM};
    use crate::titlestyle::TitleLook as _;
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
    fn the_chrome_is_painted_from_the_theme_and_from_nowhere_else() {
        // The whole point of the flavour work: a palette chosen by the user has
        // to reach this crate's chrome. A hex kept here would be a second
        // palette, and the window would keep one flavour while the buttons it
        // hosts wore another.
        let mut theme = Theme::default();
        theme.mode = ThemeMode::Dark;
        theme.background = color(0x1e1e2e);
        theme.sidebar = color(0x181825);
        theme.primary = color(0xcba6f7);

        let tokens = Tokens::new(&theme);

        assert_eq!(tokens.bg, color(0x1e1e2e));
        assert_eq!(tokens.surface, color(0x181825));
        assert_eq!(
            tokens.accent,
            color(0xcba6f7),
            "the accent a flavour states is the accent the window draws"
        );
    }

    #[test]
    fn the_two_modes_do_not_share_a_background() {
        let mut dark_theme = Theme::default();
        dark_theme.mode = ThemeMode::Dark;
        dark_theme.background = color(0x1e1e2e);
        dark_theme.foreground = color(0xcdd6f4);
        let mut light_theme = Theme::default();
        light_theme.mode = ThemeMode::Light;
        light_theme.background = color(0xeff1f5);
        light_theme.foreground = color(0x4c4f69);

        let dark = Tokens::new(&dark_theme);
        let light = Tokens::new(&light_theme);

        assert_ne!(
            dark.bg, light.bg,
            "light mode must not paint the dark palette"
        );
        assert_ne!(dark.ink, light.ink, "text must follow the theme as well");
    }

    #[test]
    fn the_accent_as_text_steps_away_from_the_bed_it_sits_on() {
        // The accent is a fill, not a letter. On a dark theme the words in it
        // have to go lighter to clear AA; on a light theme, darker.
        let mut dark_theme = Theme::default();
        dark_theme.mode = ThemeMode::Dark;
        dark_theme.primary = color(0x7c6cf0);
        let mut light_theme = dark_theme.clone();
        light_theme.mode = ThemeMode::Light;

        let dark = Tokens::new(&dark_theme);
        let light = Tokens::new(&light_theme);

        assert!(dark.accent_ink.l > dark.accent.l);
        assert!(light.accent_ink.l < light.accent.l);
    }

    #[test]
    fn spacing_is_a_multiple_of_four_and_radii_are_the_three_steps() {
        assert_eq!(PAGE_PAD % 4., 0.);
        assert_eq!(RADIUS_SM, 8.);
        assert_eq!(RADIUS_MD, 12.);
        assert_eq!(RADIUS_PILL, 999.);
    }

    #[test]
    fn the_glyph_a_key_receives_is_the_accent_the_window_shows() {
        // The initial of a key is drawn twice — as an element on the tile and as
        // a picture for the deck — and the two have to be the same colour, or a
        // key would change tone when it is sent. The picture is painted from the
        // flavour's own accent, so that is what the cache starts with.
        assert_eq!(
            crate::icons::GLYPH_ACCENT,
            crate::flavour::default().accent_rgb(),
            "the deck's pictures and the window must start on one accent"
        );
    }

    #[test]
    fn a_title_in_the_window_is_drawn_at_the_size_the_user_chose() {
        // The user's number is pixels on the deck's tile, so a window tile of
        // that same size shows exactly that number; on a smaller tile the label
        // scales down with it, and it never drops below the micro-label step
        // where no type in the window would be legible.
        let style = crate::titlestyle::TitleStyle::default();
        assert!(
            (style.window_font(108.0, 108.0) - f32::from(style.size_px)).abs() < 0.01,
            "a window tile the size of the deck's shows the chosen number"
        );
        assert!(
            style.window_font(54.0, 108.0) < f32::from(style.size_px),
            "a smaller window tile draws a smaller label"
        );
        assert!(
            style.window_font(1.0, 108.0) >= 7.0,
            "a tiny window tile still draws a legible label"
        );
    }
}

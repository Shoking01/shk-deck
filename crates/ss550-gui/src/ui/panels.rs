//! The `Actions` and `Settings` pages.
//!
//! Each page is a stack of cards in the same language the `Deck` page uses:
//! a micro-labelled title, a short line of secondary text, and controls that
//! are only offered when they can act. Nothing here decodes a picture while a
//! frame is being drawn: the key editor's picture controls paint the thumbnail
//! the key's tile is fed from, and the picture itself is chosen and imported on
//! a click.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _,
};
use std::sync::Arc;

use super::{
    activate, body, card, color, icon_label, micro_label, muted, one_line, path_line, swatch,
    titlecontrols, Tokens, FONT_CAPTION, FONT_MICRO, FONT_SUB, RADIUS_MD, RADIUS_SM,
};
use crate::{flavour, ms_text, Controller};

// ── Actions ─────────────────────────────────────────────────────────────

/// What each key runs when it is pressed on the deck.
///
/// The overview: every key as a chip, the editor for the one the window is
/// looking at — the action and the picture the key shows — and what is bound
/// right now. The editor is [`super::editor::key_editor`] and
/// [`super::editor::picture_block`], the same two widgets the `Deck` page's
/// inspector draws, so there is one implementation of "edit this key" and not
/// two.
pub(crate) fn actions_page(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let has_deck = controller.session.is_some();
    let selected = controller.selected_key;

    let picker = key_picker(controller, tokens, cx);
    let picker_card = card(tokens).w_full().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(icon_label(IconName::Frame, "KEYS", tokens))
            .child(muted(
                match has_deck {
                    true => format!("Editing key #{}", selected.get()),
                    false => "No deck connected".to_string(),
                },
                tokens,
            )),
    );

    let picker_card = match has_deck {
        true => picker_card.child(picker),
        false => picker_card.child(placeholder(
            "No deck connected",
            "Bindings belong to a deck: the rows and the key numbers come from whichever one \
             answers. Plug it in and this page fills itself in.",
            tokens,
        )),
    };

    let editor_card = card(tokens)
        .w_full()
        .child(super::editor::editor_heading(selected, tokens))
        .child(super::editor::key_editor(controller, tokens, cx))
        // What the key shows is part of what it is: the picture controls sit in
        // the same card as the action, so a key is edited in one place.
        .child(super::editor::picture_block(controller, tokens, cx));

    div()
        .flex()
        .flex_col()
        .gap(px(24.))
        .w_full()
        .child(picker_card)
        .child(editor_card)
        .child(bound_card(controller, tokens, cx))
}

/// Every key the deck reports, one row of chips per display row.
fn key_picker(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let mut rows: Vec<Div> = Vec::new();
    let selected = controller.selected_key;

    if let Some(session) = controller.session.as_ref() {
        for keys in session.rows() {
            let mut chips = Vec::new();
            for &index in keys {
                let Some(number) = crate::binding_number(session, index) else {
                    continue;
                };
                let bound = controller.store.get(number).is_some();
                let active = number == selected;
                let chip = div()
                    .id(SharedString::from(format!("picker-key-{index}")))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(2.))
                    .w(px(52.))
                    .h(px(44.))
                    .rounded(px(RADIUS_SM))
                    .border_2()
                    .border_color(if active { tokens.accent } else { tokens.border })
                    .bg(if active {
                        tokens.accent_tint
                    } else if bound {
                        tokens.elevated
                    } else {
                        tokens.surface
                    })
                    .text_size(px(FONT_SUB))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if active {
                        tokens.accent_ink
                    } else {
                        tokens.ink
                    })
                    .child(number.get().to_string())
                    .child(
                        div()
                            // The smallest step of the type scale, not a size of
                            // its own: a chip's sub-label is a micro-label.
                            .text_size(px(FONT_MICRO))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if bound {
                                tokens.soft
                            } else {
                                tokens.soft.opacity(0.5)
                            })
                            .child(if bound { "BOUND" } else { "—" }),
                    )
                    .hover(|style| style.bg(tokens.hover));

                chips.push(
                    activate(chip, cx, move |this, window, cx| {
                        this.select_key(number, window, cx);
                        cx.notify();
                    })
                    .into_any_element(),
                );
            }
            rows.push(div().flex().flex_row().gap(px(8.)).children(chips));
        }
    }

    div().flex().flex_col().gap(px(8.)).w_full().children(rows)
}

/// What is bound right now, one row per key.
fn bound_card(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let mut bound: Vec<(u8, String)> = controller
        .store
        .bindings()
        .iter()
        .map(|binding| (binding.key.get(), binding.action.summary()))
        .collect();
    bound.sort_by_key(|(number, _)| *number);

    let count = bound.len();
    let mut rows: Vec<AnyElement> = Vec::new();
    for (number, summary) in bound {
        let label = SharedString::from(format!("#{number}"));
        let row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .w_full()
            .min_w(px(0.))
            .h(px(36.))
            .px(px(8.))
            .rounded(px(RADIUS_SM))
            .bg(tokens.elevated)
            .border_1()
            .border_color(tokens.border)
            .child(
                div()
                    .flex_none()
                    .w(px(40.))
                    .text_size(px(FONT_MICRO))
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens.accent_ink)
                    .child(label),
            )
            .child(one_line(summary, FONT_SUB, tokens.ink));

        let key = ss550_engine::KeyNumber::new(number);
        let remove = match key {
            Ok(key) => Button::new(SharedString::from(format!("unbind-{number}")))
                .ghost()
                .compact()
                .icon(IconName::Delete)
                .label("REMOVE")
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.remove_binding(key, window, cx);
                    cx.notify();
                }))
                .into_any_element(),
            Err(_) => div().into_any_element(),
        };

        rows.push(
            row.child(div().flex_none().child(remove))
                .into_any_element(),
        );
    }

    card(tokens)
        .w_full()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(icon_label(IconName::Inbox, "BOUND RIGHT NOW", tokens))
                .child(muted(
                    format!("{count} key{}", if count == 1 { "" } else { "s" }),
                    tokens,
                )),
        )
        .child(match rows.is_empty() {
            true => placeholder(
                "No key bound yet",
                "Pick a key above, choose what it runs, then press SAVE BINDING. A key that \
                 runs nothing still lights up, and says so in the status bar.",
                tokens,
            ),
            false => div().flex().flex_col().gap(px(8.)).children(rows),
        })
}

// ── Settings ────────────────────────────────────────────────────────────

/// One flavour in the appearance card: its accent, its name, what it looks like,
/// and the mode it carries.
///
/// The whole row is the target, not a radio button inside it: a list of palettes
/// is a list of things to try, and a hit area the width of the card is what makes
/// trying them cheap. The active one is ringed and ticked, so the state is said
/// twice — colour and mark — and never by colour alone.
fn flavour_row(
    controller: &mut Controller,
    flavour: &'static flavour::Flavour,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> AnyElement {
    let active = controller.flavour.name == flavour.name;
    let mode = match flavour.mode.is_dark() {
        true => "DARK",
        false => "LIGHT",
    };

    let mut row = div()
        .id(SharedString::from(format!("flavour-{}", flavour.name)))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .w_full()
        .min_w(px(0.))
        .p(px(10.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if active { tokens.accent } else { tokens.border })
        .bg(if active {
            tokens.accent_tint
        } else {
            tokens.elevated
        })
        .hover(|style| style.bg(tokens.hover).border_color(tokens.accent))
        .child(swatch(tokens, flavour.accent, 14.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(FONT_SUB))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if active {
                            tokens.accent_ink
                        } else {
                            tokens.ink
                        })
                        .child(SharedString::from(flavour.name)),
                )
                .child(one_line(flavour.note, FONT_CAPTION, tokens.soft)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .flex_none()
                .child(micro_label(mode, tokens)),
        );

    if active {
        row = row.child(
            Icon::new(IconName::Check)
                .size(px(14.))
                .text_color(tokens.accent_ink),
        );
    }

    activate(row, cx, move |this, _window, cx| {
        this.set_flavour(flavour, cx);
    })
    .into_any_element()
}

/// How the controller behaves when the window is closed, and where it keeps
/// what it knows.
pub(crate) fn settings_page(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let mono = cx.theme().mono_font_family.clone();
    let tray_note = match &controller.tray_error {
        Some(error) => format!("The tray icon is missing: {error}"),
        None => "The tray icon is in the notification area.".to_string(),
    };

    // ── Startup ──
    let mut startup = card(tokens)
        .w_full()
        .child(icon_label(IconName::Bot, "STARTUP", tokens))
        .child(
            Switch::new("start-with-windows")
                .checked(controller.starts_with_windows)
                // The library's own "on" colour is the theme's near-white, which
                // in dark mode reads as a disabled control next to the accent
                // used by every other choice in the window.
                .color(tokens.accent)
                .label("START WITH WINDOWS")
                .on_change({
                    let controller = cx.entity();
                    move |checked: &bool, _window, cx| {
                        controller.update(cx, |this, cx| this.set_autostart(*checked, cx));
                    }
                }),
        )
        .child(muted(
            "Turn this on and Windows starts the controller when you sign in, hidden in the \
             tray. It copies the controller to its own folder under %LOCALAPPDATA%, writes one \
             entry under HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run, for this user \
             only, and removes that entry again when you turn this off.",
            tokens,
        ));

    if let Some(note) = &controller.autostart_note {
        startup = startup.child(muted(note.clone(), tokens));
    }
    // The one place the user learns that the copy Windows starts is stale, or
    // that it could not be replaced at all.
    if let Some(note) = &controller.sign_in_copy {
        // A sentence that says the copies agree must not be painted like a
        // warning: the tone is what separates "nothing to do" from "act on this".
        let tone = if note.problem {
            tokens.bad
        } else {
            tokens.good
        };
        startup = startup.child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(8.))
                .w_full()
                .min_w(px(0.))
                .p(px(8.))
                .rounded(px(RADIUS_SM))
                .bg(tokens.elevated)
                .border_1()
                .border_color(tokens.border)
                .child(super::dot(tokens, tone, 8.))
                .child(one_line(note.text.clone(), FONT_CAPTION, tokens.soft)),
        );
    }

    // ── Appearance ──
    let mut appearance = card(tokens)
        .w_full()
        .child(icon_label(IconName::Palette, "APPEARANCE", tokens))
        .child(muted(
            "A flavour repaints the whole window at once: this chrome and the controls on it. A \
             key whose initial is drawn on the accent is repainted on the deck too, so the two \
             never show different colours.",
            tokens,
        ));

    let mut flavours = div().flex().flex_col().gap(px(6.)).w_full();
    for flavour in flavour::FLAVOURS {
        flavours = flavours.child(flavour_row(controller, flavour, tokens, cx));
    }
    appearance = appearance.child(flavours);

    // ── Files ──
    let bindings = match &controller.store_path {
        Some(path) => path.display().to_string(),
        None => "Saved per deck, once one answers".to_string(),
    };
    let files = card(tokens)
        .w_full()
        .child(icon_label(IconName::FolderOpen, "FILES", tokens))
        .child(info_row(
            "CONFIG",
            path_line(
                ss550_engine::config_dir().display().to_string(),
                tokens,
                &mono,
            ),
            tokens,
        ))
        .child(info_row(
            "PICTURES",
            path_line(
                controller.assets.root().display().to_string(),
                tokens,
                &mono,
            ),
            tokens,
        ))
        .child(info_row(
            "BINDINGS",
            path_line(bindings, tokens, &mono),
            tokens,
        ))
        .child(info_row(
            "INSTALLED",
            path_line(
                match ss550_tray::install::installed_executable() {
                    Ok(path) => path.display().to_string(),
                    Err(error) => error.to_string(),
                },
                tokens,
                &mono,
            ),
            tokens,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .w_full()
                .pt(px(4.))
                .child(
                    Button::new("install-app")
                        .primary()
                        .icon(IconName::Star)
                        .label("INSTALL TO START MENU")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.install_app();
                            cx.notify();
                        })),
                )
                .child(muted(
                    "Copies the controller to the folder above and puts an entry in this \
                     user's Start Menu, so Windows Search and Flow Launcher find it. \
                     Starting it again while it runs brings the window back.",
                    tokens,
                )),
        )
        .child(one_line(
            "Paths are cut short from the left, so the name at the end stays visible.",
            FONT_CAPTION,
            tokens.soft,
        ));

    // ── About ──
    let about = card(tokens)
        .w_full()
        .child(icon_label(IconName::Info, "ABOUT", tokens))
        .child(info_row(
            "VERSION",
            body(format!("v{}", env!("CARGO_PKG_VERSION")), tokens),
            tokens,
        ))
        .child(info_row(
            "FRAME",
            body(
                format!("{} ms/frame", ms_text(controller.last_frame)),
                tokens,
            ),
            tokens,
        ))
        .child(info_row(
            "TRAY",
            one_line(tray_note, FONT_SUB, tokens.soft),
            tokens,
        ));

    div()
        .flex()
        .flex_col()
        .gap(px(24.))
        .w_full()
        .child(startup)
        .child(appearance)
        .child(thresholds_card(controller, tokens, cx))
        .child(titles_card(controller, tokens, cx))
        .child(files)
        .child(about)
}

/// The lines the Stats page paints a card by.
///
/// Two rows of steppers rather than text fields: a threshold is a number in a
/// small range, and a field would need a parser, a refusal and a message for
/// every way a number can be wrong. A click is unambiguous, and the pair is kept
/// valid in one place (`Lines::new`), so a warning can never end up above its
/// limit however many times it is pressed.
fn thresholds_card(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let mut card_body = card(tokens)
        .w_full()
        .child(icon_label(IconName::TriangleAlert, "THRESHOLDS", tokens))
        .child(muted(
            "Where the Stats page turns a card amber, and then red.",
            tokens,
        ));

    // What went wrong with the file, next to the controls that write it: the
    // status line carries the same sentence for a moment, but it is replaced the
    // next time the deck reports in.
    if let Some(note) = &controller.thresholds_note {
        card_body = card_body.child(
            div()
                .text_size(px(FONT_MICRO))
                .text_color(tokens.warn)
                .child(note.clone()),
        );
    }

    for kind in [
        crate::thresholds::Kind::Share,
        crate::thresholds::Kind::Degrees,
    ] {
        let lines = controller.thresholds.lines(kind);
        let ceiling = crate::thresholds::Thresholds::ceiling(kind);
        let step = if kind == crate::thresholds::Kind::Share {
            5.0
        } else {
            2.0
        };
        // The two lines on one row, each one a caption followed by its own pair of
        // arrows with the number between them: `WARN − 80 +` is the shape every
        // stepper has ever had, and it needs no second explanation.
        let row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(18.))
            .w_full()
            .child(
                div()
                    .w(px(110.))
                    .text_size(px(FONT_MICRO))
                    .text_color(tokens.ink)
                    .child(crate::thresholds::Thresholds::label(kind)),
            )
            .child(line_control(
                kind,
                "warn",
                "WARN",
                lines.warn,
                tokens.warn,
                step,
                ceiling,
                tokens,
                cx,
            ))
            .child(line_control(
                kind,
                "alarm",
                "LIMIT",
                lines.alarm,
                tokens.bad,
                step,
                ceiling,
                tokens,
                cx,
            ));
        card_body = card_body.child(row);
    }
    card_body
}

/// `WARN − 80 +`, one line of one kind.
#[allow(clippy::too_many_arguments)]
fn line_control(
    kind: crate::thresholds::Kind,
    which: &'static str,
    caption: &'static str,
    value: f32,
    colour: Hsla,
    step: f32,
    ceiling: f32,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(FONT_MICRO))
                .text_color(tokens.soft)
                .child(caption),
        )
        .child(stepper(kind, which, value, -step, ceiling, cx))
        .child(
            div()
                .w(px(34.))
                .text_size(px(FONT_CAPTION))
                .text_color(colour)
                .child(format!("{value:.0}")),
        )
        .child(stepper(kind, which, value, step, ceiling, cx))
}

/// One arrow that moves a line by `step` and saves it.
///
/// The value is carried in the button's own id, so two arrows on the same line
/// never share an element.
fn stepper(
    kind: crate::thresholds::Kind,
    which: &'static str,
    value: f32,
    step: f32,
    ceiling: f32,
    cx: &mut Context<Controller>,
) -> AnyElement {
    let target = value + step;
    let label = if step > 0.0 { "+" } else { "−" };
    // The kind is part of the id because both kinds can sit on the same number
    // and two elements with one id are one element as far as the window is
    // concerned.
    let scope = match kind {
        crate::thresholds::Kind::Share => "share",
        crate::thresholds::Kind::Degrees => "degrees",
    };
    let id = SharedString::from(format!("{scope}-{which}-{}-{label}", value as i32));
    Button::new(id)
        .ghost()
        .label(label)
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            let lines = this.thresholds.lines(kind);
            let next = if which == "warn" {
                crate::thresholds::Lines::new(target, lines.alarm, ceiling)
            } else {
                crate::thresholds::Lines::new(lines.warn, target, ceiling)
            };
            this.set_thresholds(this.thresholds.with(kind, next), cx);
        }))
        .into_any_element()
}

/// How a title is drawn when its key does not say otherwise.
///
/// The **default** style, applied on the click: the controller recomposes the
/// payloads of the keys that follow it and repaints those, so the deck itself is
/// the preview and no second renderer exists to keep in step. A key with a style
/// of its own is untouched by this card — that is the difference between the two
/// — and the card is offered with no deck too, because the default is saved
/// whether or not anything is plugged in.
fn titles_card(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    // How many keys say something of their own, bounded by the deck's own key
    // count: the profile is keyed by key number and nothing else knows how many
    // there are.
    let key_count = controller
        .session
        .as_ref()
        .map_or(0, crate::session::DeckSession::key_count);
    // The page the deck is showing: a folder may set styles the root never did.
    let overrides = match controller.current_keys() {
        Some(keys) => (1..=key_count as u16)
            .filter(|key| keys.title_style(*key).is_some())
            .count(),
        None => 0,
    };

    card(tokens)
        .w_full()
        .child(icon_label(
            IconName::CaseSensitive,
            "TITLE DEFAULTS",
            tokens,
        ))
        .child(muted(
            "How a title is drawn when the key does not say otherwise: the words are set in a \
             font the system already has, at a pixel size, and a title too long for its band steps \
             down rather than being cut. A key can carry a style of its own, from the editor on \
             the key itself.",
            tokens,
        ))
        .child(titlecontrols::rows(
            controller,
            crate::FontTarget::Defaults,
            tokens,
            cx,
        ))
        .child(muted(
            match overrides {
                0 => "No key has a style of its own, so every title follows this.".to_string(),
                1 => "One key has a style of its own and does not follow this.".to_string(),
                count => format!("{count} keys have a style of their own and do not follow this."),
            },
            tokens,
        ))
        .child(muted(
            "The size is a ceiling: the fit brings it down until the words fit, and never up. \
             Every choice is saved for the next start.",
            tokens,
        ))
}
// ── Shared pieces for the pages ─────────────────────────────────────────

/// A 44 px thumbnail: the fitted tile, or a quiet box when it is not ready.
///
/// One box, so a picture looks the same wherever the window shows it small: the
/// key's own picture in the editor, and the icon of an application in the
/// picker.
pub(crate) fn thumbnail(picture: Option<Arc<gpui_kit::RenderImage>>, tokens: Tokens) -> Div {
    let boxed = div()
        .flex_none()
        .w(px(44.))
        .h(px(44.))
        .rounded(px(6.))
        .overflow_hidden()
        .bg(color(0x000000))
        .border_1()
        .border_color(tokens.border);

    match picture {
        Some(picture) => boxed.child(gpui_kit::img(picture).w(px(44.)).h(px(44.))),
        None => boxed.bg(tokens.surface),
    }
}

/// A framed explanation, used wherever a list would be empty.
pub(crate) fn placeholder(title: &'static str, text: &str, tokens: Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .w_full()
        .min_h(px(140.))
        .p(px(24.))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(tokens.border)
        .border_dashed()
        .bg(tokens.elevated)
        .child(
            div()
                .text_size(px(FONT_SUB))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.ink)
                .child(title),
        )
        .child(
            div()
                .text_size(px(FONT_CAPTION))
                .text_color(tokens.soft)
                .text_align(gpui_kit::TextAlign::Center)
                .child(SharedString::from(text.to_string())),
        )
}

/// A label and a value on one line: the shape every Settings row uses.
fn info_row(label: &'static str, value: impl IntoElement, tokens: Tokens) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .w_full()
        .min_w(px(0.))
        .child(
            div()
                .flex_none()
                .w(px(88.))
                .text_size(px(FONT_MICRO))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.soft)
                .child(label),
        )
        .child(div().flex_1().min_w(px(0.)).child(value))
}

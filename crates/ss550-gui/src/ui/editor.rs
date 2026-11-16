//! The one widget that edits what a key runs.
//!
//! The `Deck` page's inspector and the `Actions` page both draw this, so there
//! is exactly one implementation of "edit this key": the three kinds, the field
//! each kind needs, SAVE BINDING and CLEAR KEY. A second copy is how the two
//! places end up disagreeing about what a field means.
//!
//! A palette template dropped on a key opens this editor where the key is — on
//! the page the key is on, with the first field focused. The drop never moves
//! the window and never writes: the key keeps what it had until SAVE BINDING.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::Field;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _,
};

use super::panels::{placeholder, thumbnail};
use super::{
    activate, micro_label, one_line, path_line, segment, Tokens, FONT_CAPTION, FONT_SUB, RADIUS_SM,
};
use crate::apps;
use crate::{Controller, DraftKind};

/// How tall the list of installed applications is before it scrolls.
///
/// Tall enough for six or seven rows, which is what a person reads before
/// typing into the search field instead.
const APP_LIST_H: f32 = 240.;

/// How many matching applications the list draws at once.
///
/// A cap on the work a frame does, not on what the user can reach: the search
/// field narrows the list, and a Start Menu with more than this many matches for
/// one query is a query worth typing more of.
const APP_ROWS: usize = 40;

/// The words a keystroke is written with.
///
/// Stated once, under the field, because the engine refuses anything outside it
/// and a refusal is only useful next to the list of what would have been
/// accepted. It is the engine's own vocabulary, not a second copy of the rules:
/// the parser is still the only thing that decides.
const KEY_VOCABULARY: &str = "Modifiers: ctrl alt shift win · Keys: a–z, 0–9, f1–f24, arrows, \
                              enter escape tab space backspace delete home end pageup pagedown \
                              insert";

/// The editor for the selected key: kind, fields, and the two buttons.
///
/// The caller supplies the card and its heading; this is the form and nothing
/// else, so both places show the same controls in the same order.
pub(crate) fn key_editor(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let has_deck = controller.session.is_some();
    let draft = controller.draft;

    let kind = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(8.))
        .child(segment(
            SharedString::from("draft-launch"),
            "RUN APP",
            draft == DraftKind::Launch,
            tokens,
            cx,
            |this, _window, cx| {
                this.draft = DraftKind::Launch;
                this.note = "Fill the program path, then press SAVE BINDING.".into();
                cx.notify();
            },
        ))
        .child(segment(
            SharedString::from("draft-url"),
            "OPEN URL",
            draft == DraftKind::OpenUrl,
            tokens,
            cx,
            |this, _window, cx| {
                this.draft = DraftKind::OpenUrl;
                this.note = "Fill the URL, then press SAVE BINDING.".into();
                cx.notify();
            },
        ))
        .child(segment(
            SharedString::from("draft-keys"),
            "KEYSTROKES",
            draft == DraftKind::Keys,
            tokens,
            cx,
            |this, _window, cx| {
                this.draft = DraftKind::Keys;
                this.note = "Fill the combination, then press SAVE BINDING.".into();
                cx.notify();
            },
        ));

    let launch_fields = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .child(
            Field::new()
                .label("PROGRAM · full path to the program to run")
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .child(Input::new(&controller.program_input)),
                        )
                        // The picture the key would wear, from the same cache the
                        // tile and the deck are fed from: choosing an application
                        // shows its icon before SAVE BINDING.
                        .child(program_preview(controller, tokens)),
                ),
        )
        .child(app_picker(controller, tokens, cx))
        .child(
            Field::new()
                .label("ARGUMENTS · one per line, each passed on its own")
                .child(Textarea::new(&controller.args_input)),
        );

    let url_fields = div().flex().flex_col().gap(px(8.)).w_full().child(
        Field::new()
            .label("URL · must start with its scheme, e.g. https://")
            .child(Input::new(&controller.url_input)),
    );

    // One field, because a combination is one thing: the modifiers and the key
    // are parsed together, and splitting them across controls would let the
    // window hold half a keystroke it cannot send.
    let key_fields = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .child(
            Field::new()
                .label("KEYS · the combination to send, e.g. ctrl+shift+m")
                .child(Input::new(&controller.keys_input)),
        )
        .child(
            div()
                .text_size(px(FONT_CAPTION))
                .text_color(tokens.soft)
                .child(KEY_VOCABULARY),
        );

    let fields = match draft {
        DraftKind::Launch => launch_fields,
        DraftKind::OpenUrl => url_fields,
        DraftKind::Keys => key_fields,
    };

    let save_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .child(
            Button::new("save-binding")
                .primary()
                .label("SAVE BINDING")
                .disabled(!has_deck)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.save_binding(cx);
                    cx.notify();
                })),
        )
        // Quiet on purpose: clearing a key is destructive, and a red button in
        // the middle of a form invites the click it punishes.
        .child(
            Button::new("clear-binding")
                .ghost()
                .label("CLEAR KEY")
                .disabled(!has_deck)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.clear_binding();
                    cx.notify();
                })),
        );

    let saved_to = match (&controller.store_path, has_deck) {
        (Some(path), _) => format!("Bindings are saved to {}.", path.display()),
        (None, true) => "Bindings are saved once the deck answers.".to_string(),
        (None, false) => {
            "Bindings are saved per deck, in the app's own configuration folder.".to_string()
        }
    };

    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .w_full()
        .child(kind)
        .child(fields)
        .child(save_row)
        .child(one_line(saved_to, FONT_CAPTION, tokens.soft))
}

/// The heading the editor is introduced with, so both callers say it the same.
pub(crate) fn editor_heading(number: ss550_engine::KeyNumber, tokens: Tokens) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(micro_label("WHAT THIS KEY RUNS", tokens))
        .child(micro_label(format!("#{}", number.get()), tokens))
}

/// The picture the key would wear for the program in the field.
///
/// A lookup in the icon cache, which was filled when the draft changed — so this
/// draws a picture and never extracts one. No picture means the program has no
/// icon to read, and the box stays empty: the tile will draw the initial.
fn program_preview(controller: &Controller, tokens: Tokens) -> Div {
    let picture = controller
        .draft_program
        .as_deref()
        .and_then(|program| controller.icons.program_frame(program))
        .cloned();
    thumbnail(picture, tokens)
}

/// The applications installed on this PC, offered under the `PROGRAM` field.
///
/// The list is a click away and nothing more: opening it asks for the scan, and
/// the scan is the only slow step in this form. Choosing a row fills the field
/// and closes the list; typing a path by hand keeps working exactly as it did,
/// with the field untouched by any of this.
fn app_picker(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let open = controller.app_picker_open;
    let mut section = div().flex().flex_col().gap(px(8.)).w_full().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                Button::new("toggle-app-picker")
                    .ghost()
                    .label(match open {
                        true => "HIDE THE LIST",
                        false => "CHOOSE AN APP",
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.toggle_app_picker(cx);
                        cx.notify();
                    })),
            )
            .child(one_line(
                match open {
                    true => "Pick one and the path fills itself.",
                    false => "Or type the path yourself.",
                },
                FONT_CAPTION,
                tokens.soft,
            )),
    );

    if !open {
        return section;
    }

    // Everything the list draws is resolved before a widget is built: a row's
    // click handler needs the entity context, and the list is borrowed while it
    // is being read. The search runs here, over the cached names — no scan, no
    // filesystem, no shell.
    let query = controller.app_query.read(cx).value().trim().to_lowercase();
    let mut entries: Vec<(
        usize,
        SharedString,
        String,
        Option<Arc<gpui_kit::RenderImage>>,
    )> = Vec::new();
    if let Some(list) = controller.app_list.list() {
        for (index, app) in list.iter().enumerate() {
            if entries.len() == APP_ROWS {
                break;
            }
            if app.matches(&query) {
                entries.push((
                    index,
                    SharedString::from(app.name.clone()),
                    app.target.display().to_string(),
                    app.icon.clone(),
                ));
            }
        }
    }

    let rows: Vec<AnyElement> = entries
        .into_iter()
        .map(|(index, name, target, icon)| {
            let row = div()
                .id(SharedString::from(format!("app-row-{index}")))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .w_full()
                .min_w(px(0.))
                .p(px(8.))
                .rounded(px(RADIUS_SM))
                .border_1()
                .border_color(tokens.border)
                .bg(tokens.elevated)
                .child(thumbnail(icon, tokens))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .flex_1()
                        .min_w(px(0.))
                        .child(one_line(name, FONT_SUB, tokens.ink))
                        .child(path_line(target, tokens, &cx.theme().mono_font_family)),
                )
                .hover(|style| style.bg(tokens.hover));

            activate(row, cx, move |this, window, cx| {
                this.choose_app(index, window, cx);
            })
            .into_any_element()
        })
        .collect();

    let body: AnyElement = match &controller.app_list {
        apps::Apps::Unscanned => placeholder(
            "The list has not been scanned yet",
            "Finding the applications means reading every shortcut in the Start Menu, one at a \
             time. It happens when you ask, not while the window is open.",
            tokens,
        )
        .child(
            Button::new("scan-apps")
                .primary()
                .label("SCAN")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.scan_apps(cx);
                    cx.notify();
                })),
        )
        .into_any_element(),
        apps::Apps::Scanning => placeholder(
            "Looking through the Start Menu…",
            "The list fills in as soon as the scan finishes. Nothing else in the window waits \
             for it.",
            tokens,
        )
        .into_any_element(),
        apps::Apps::Ready(list) if list.is_empty() => placeholder(
            "No application was found",
            "The Start Menu of this PC holds no shortcut that leads to a program. A path can \
             still be typed in the field above.",
            tokens,
        )
        .into_any_element(),
        apps::Apps::Ready(list) => {
            let count = list.len();
            let shown = rows.len();
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .w_full()
                .child(
                    Field::new()
                        .label(format!(
                            "SEARCH · {count} application{} installed",
                            if count == 1 { "" } else { "s" }
                        ))
                        .child(Input::new(&controller.app_query)),
                )
                .child(match shown {
                    0 => one_line(
                        "No application matches that. Clear the search to see them all.",
                        FONT_CAPTION,
                        tokens.soft,
                    )
                    .into_any_element(),
                    _ => div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .w_full()
                        .max_h(px(APP_LIST_H))
                        .overflow_y_scrollbar()
                        .children(rows)
                        .into_any_element(),
                })
                .into_any_element()
        }
    };

    section = section.child(body);
    section
}

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

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::Field;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::Disableable as _;
use gpui_kit::{div, px, ClickEvent, Context, Div, ParentElement as _, SharedString, Styled as _};

use super::{micro_label, one_line, segment, Tokens, FONT_CAPTION};
use crate::{Controller, DraftKind};

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
                .child(Input::new(&controller.program_input)),
        )
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

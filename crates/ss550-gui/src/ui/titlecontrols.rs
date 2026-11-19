//! The controls that describe one title style.
//!
//! The same block appears twice: under `TITLE DEFAULTS` on the settings page,
//! where it edits the style every key without one follows, and in the editor for
//! the selected key, where it edits that key's own style in its profile. Both
//! call [`rows`], so the two cannot drift into two vocabularies for the same
//! four choices — and one font list serves both, because [`FontTarget`] says
//! which style the open list is for.
//!
//! Nothing here measures or draws text: a click writes the style, the controller
//! recomposes the payloads that changed, and the key on the desk is the preview.

use gpui_kit::component::input::Input;
use gpui_kit::{
    div, px, AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _,
};

use super::{
    activate, one_line, segment, shorten, Tokens, FONT_CAPTION, FONT_MICRO, FONT_SUB, RADIUS_SM,
};
use crate::titlestyle::{position_label, position_word, TitleLook as _, TitlePosition, TitleStyle};
use crate::{Controller, FontTarget};

/// How many font families the list shows at once.
///
/// A machine can have hundreds; the list is a filter box, not an inventory, so
/// it stops at the first rows that match what was typed.
const FONT_ROWS: usize = 6;

/// The four choices, for one target's style, stacked.
pub(crate) fn rows(
    controller: &mut Controller,
    target: FontTarget,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let style = controller.title_style_of(target);

    let font_row = row(
        "FONT",
        vec![segment(
            SharedString::from(format!("title-font-{}", target_id(target))),
            shorten(style.family_label(), 24).to_uppercase(),
            controller.font_picker == Some(target),
            tokens,
            cx,
            move |this: &mut Controller, _window, cx| {
                this.toggle_font_picker(target, cx);
            },
        )],
        tokens,
    );

    let font_list = match controller.font_picker == Some(target) {
        false => None,
        // The scan is still running. It is off the UI thread, so the window is
        // answering; the list fills itself when the fonts arrive.
        true if controller.text.is_none() => Some(div().flex().w_full().child(one_line(
            "Reading the system's fonts…",
            FONT_CAPTION,
            tokens.soft,
        ))),
        true => {
            let query = controller.font_query.read(cx).value().trim().to_lowercase();
            let mut choices: Vec<AnyElement> = Vec::new();
            // The system's own font is a row like any other. It is what an
            // empty family means, and it is the one choice every machine has.
            if "system default".contains(&query) {
                choices.push(font_choice("", "SYSTEM DEFAULT", &style, tokens, cx));
            }
            for family in &controller.font_families {
                if choices.len() >= FONT_ROWS {
                    break;
                }
                if !family.to_lowercase().contains(&query) {
                    continue;
                }
                choices.push(font_choice(family, family, &style, tokens, cx));
            }
            Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .w_full()
                    .child(Input::new(&controller.font_query))
                    .children(choices),
            )
        }
    };

    let size_row = row(
        "SIZE",
        vec![
            segment(
                SharedString::from(match target {
                    FontTarget::Defaults => "title-size-down-defaults".to_string(),
                    FontTarget::Key(key) => format!("title-size-down-{key}"),
                }),
                "-",
                false,
                tokens,
                cx,
                move |this: &mut Controller, _window, cx| {
                    let px = this.title_style_of(target).size_px.saturating_sub(1);
                    this.set_title_size(target, px, cx);
                },
            ),
            div()
                .flex_none()
                .w(px(56.))
                .text_size(px(FONT_SUB))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.ink)
                .child(format!("{} px", style.size_px))
                .into_any_element(),
            segment(
                SharedString::from(match target {
                    FontTarget::Defaults => "title-size-up-defaults".to_string(),
                    FontTarget::Key(key) => format!("title-size-up-{key}"),
                }),
                "+",
                false,
                tokens,
                cx,
                move |this: &mut Controller, _window, cx| {
                    let px = this.title_style_of(target).size_px.saturating_add(1);
                    this.set_title_size(target, px, cx);
                },
            ),
        ],
        tokens,
    );

    let position_row = row(
        "POSITION",
        TitlePosition::ALL
            .iter()
            .map(|&position| {
                segment(
                    SharedString::from(format!(
                        "title-position-{}-{}",
                        position_word(position),
                        target_id(target)
                    )),
                    position_label(position),
                    style.position == position,
                    tokens,
                    cx,
                    move |this: &mut Controller, _window, cx| {
                        let style = TitleStyle {
                            position,
                            ..this.title_style_of(target)
                        };
                        this.set_title_style_at(target, style, cx);
                    },
                )
            })
            .collect(),
        tokens,
    );

    let case_row = row(
        "CASE",
        [false, true]
            .iter()
            .map(|&uppercase| {
                segment(
                    SharedString::from(format!(
                        "title-case-{}-{}",
                        match uppercase {
                            true => "upper",
                            false => "typed",
                        },
                        target_id(target)
                    )),
                    match uppercase {
                        true => "UPPERCASE",
                        false => "AS TYPED",
                    },
                    style.uppercase == uppercase,
                    tokens,
                    cx,
                    move |this: &mut Controller, _window, cx| {
                        let style = TitleStyle {
                            uppercase,
                            ..this.title_style_of(target)
                        };
                        this.set_title_style_at(target, style, cx);
                    },
                )
            })
            .collect(),
        tokens,
    );

    let mut block = div().flex().flex_col().gap(px(8.)).w_full().child(font_row);
    if let Some(list) = font_list {
        block = block.child(list);
    }
    block.child(size_row).child(position_row).child(case_row)
}

/// The button that takes a key back to the app's default style.
///
/// Shown only for a key that has an override of its own: there is nothing to
/// remove otherwise.
pub(crate) fn reset(
    controller: &Controller,
    target: FontTarget,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Option<Div> {
    let FontTarget::Key(key) = target else {
        return None;
    };
    controller
        .profile_store
        .title_style(key, controller.current_page())?;
    let button = div()
        .id(SharedString::from(format!("title-reset-{key}")))
        .flex()
        .items_center()
        .justify_center()
        .h(px(30.))
        .px(px(12.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(tokens.border)
        .text_size(px(FONT_MICRO))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.soft)
        .cursor_pointer()
        .child("USE THE DEFAULT STYLE")
        .hover(|style| style.bg(tokens.hover));
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .w_full()
            .child(activate(button, cx, move |this, _window, cx| {
                this.set_key_title_style(key, None, cx);
            }))
            .child(one_line(
                "The words stay; only how they are drawn goes back to the default.",
                FONT_CAPTION,
                tokens.soft,
            )),
    )
}

/// One family in the list: the row that puts it on the style being edited.
fn font_choice(
    family: &str,
    label: &str,
    style: &TitleStyle,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> AnyElement {
    let chosen = style.family == family;
    let chosen_family = family.to_string();
    let row = div()
        .id(SharedString::from(format!("font-row-{label}")))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .w_full()
        .min_w(px(0.))
        .p(px(8.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if chosen { tokens.accent } else { tokens.border })
        .bg(tokens.elevated)
        .child(one_line(label.to_string(), FONT_SUB, tokens.ink))
        .hover(|style| style.bg(tokens.hover));

    activate(row, cx, move |this, window, cx| {
        this.choose_font(&chosen_family, window, cx);
    })
    .into_any_element()
}

/// A label and its choices on one line: the shape every title choice uses.
fn row(label: &'static str, choices: Vec<AnyElement>, tokens: Tokens) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .w_full()
        .child(
            div()
                .flex_none()
                .w(px(88.))
                .text_size(px(FONT_MICRO))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.soft)
                .child(label),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .children(choices),
        )
}

/// The part of a setting id that says which style a chip belongs to.
fn target_id(target: FontTarget) -> String {
    match target {
        FontTarget::Defaults => "defaults".to_string(),
        FontTarget::Key(key) => format!("key-{key}"),
    }
}

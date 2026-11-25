//! The card that owns the deck's pages.
//!
//! A page is a folder: a set of keys the deck swaps to, and swaps back from. This
//! is where they are made, opened and removed, and where the key that opens one
//! is chosen — the editor cannot do it, because it edits what a key *runs*, and a
//! page is not something that runs.
//!
//! The navigation actions are offered here and nowhere else, so a key that
//! navigates is only ever bound from a place that knows which pages and profiles
//! exist.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::{
    div, px, AnyElement, ClickEvent, Context, Div, IntoElement, ParentElement as _, SharedString,
    Styled as _,
};

use super::{card, icon_label, muted, one_line, Tokens, FONT_CAPTION, FONT_MICRO, RADIUS_SM};
use crate::Controller;
use gpui_kit::assets::IconName;

/// Which page row a button belongs to.
///
/// A plain value rather than a closure per row: the handlers branch on this, so
/// every row is built by the same call and there are no generic parameters to
/// infer.
#[derive(Clone)]
enum Target {
    /// The profile's own keys.
    Root,
    /// A named folder.
    Page(String),
}

/// The card: the pages of the active profile, and the keys that open them.
pub(crate) fn pages_card(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let selected = controller.selected_key.get();
    let current = controller.current_page().map(str::to_string);
    let profiles = controller.profile_store.profile_names();
    let active_profile = controller.profile_store.active_name().map(str::to_string);
    let pages: Vec<String> = controller
        .profile_store
        .active()
        .map(|profile| profile.page_names().map(str::to_string).collect())
        .unwrap_or_default();

    let mut rows: Vec<AnyElement> =
        vec![row(Target::Root, "TOP PAGE", current.is_none(), tokens, cx)];
    for page in &pages {
        rows.push(row(
            Target::Page(page.clone()),
            page,
            current.as_deref() == Some(page.as_str()),
            tokens,
            cx,
        ));
    }

    let back_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .w_full()
        .child(
            Button::new("bind-back")
                .ghost()
                .label(format!("BIND BACK ON KEY #{selected}"))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.bind_selected_back(cx);
                    cx.notify();
                })),
        )
        .child(muted(
            "One step out of whatever page the key is on. On the top page it says so.",
            tokens,
        ));

    // The profiles of this deck: the other place a key can go.
    let mut profile_rows: Vec<AnyElement> = Vec::new();
    for name in &profiles {
        if active_profile.as_deref() == Some(name.as_str()) {
            continue;
        }
        let target = name.clone();
        profile_rows.push(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .w_full()
                .child(
                    Button::new(SharedString::from(format!("bind-profile-{name}")))
                        .ghost()
                        .label(format!("BIND {name} ON KEY #{selected}"))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.bind_selected_profile(&target, cx);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        );
    }

    let mut sheet = card(tokens)
        .w_full()
        .child(icon_label(IconName::BookOpen, "PAGES", tokens))
        .child(muted(
            "A page is a folder: its own keys, its own colours. A navigation key swaps the \
             deck to it and another swaps it back. Every profile starts on the top page, so a \
             folder that was removed can never strand the deck.",
            tokens,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(Input::new(&controller.page_input)),
                )
                .child(
                    Button::new("add-page")
                        .primary()
                        .icon(IconName::Plus)
                        .label("ADD PAGE")
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.add_page(window, cx);
                            cx.notify();
                        })),
                ),
        )
        .children(rows);

    if !profile_rows.is_empty() {
        sheet = sheet
            .child(muted(
                "Or send the selected key to another profile:",
                tokens,
            ))
            .children(profile_rows);
    }

    sheet.child(back_row).child(one_line(
        "The field names a new page, and renames the one whose RENAME you press. A page's own \
         keys are edited below, with that page open.",
        FONT_CAPTION,
        tokens.soft,
    ))
}

/// One row of the list: the name, whether the deck is on it, and what it offers.
fn row(
    target: Target,
    label: &str,
    is_current: bool,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> AnyElement {
    let shown = target.clone();
    let mut actions = div().flex().flex_row().items_center().gap(px(4.)).child(
        Button::new(SharedString::from(format!("page-show-{label}")))
            .ghost()
            .icon(IconName::ArrowRight)
            .label("OPEN")
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                match &shown {
                    Target::Root => this.show_deck_page(None, cx),
                    Target::Page(name) => this.show_deck_page(Some(name.as_str()), cx),
                }
                cx.notify();
            })),
    );

    // The root cannot be entered, so there is nothing to bind to it. It can be
    // bound as a *destination* only through `BACK`, which the card offers below.
    if let Target::Page(name) = &target {
        let renamed = name.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("page-rename-{label}")))
                .ghost()
                .icon(IconName::Replace)
                .label("RENAME")
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.rename_page(&renamed, window, cx);
                    cx.notify();
                })),
        );
        let bound = name.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("page-bind-{label}")))
                .ghost()
                .icon(IconName::CaseSensitive)
                .label("BIND")
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.bind_selected_to_page(&bound, cx);
                    cx.notify();
                })),
        );
        let removed = name.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("page-remove-{label}")))
                .ghost()
                .icon(IconName::Close)
                .label("X")
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.remove_page(&removed, cx);
                    cx.notify();
                })),
        );
    }

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .w_full()
        .min_w(px(0.))
        .p(px(8.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(match is_current {
            true => tokens.accent,
            false => tokens.border,
        })
        .bg(tokens.elevated)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .min_w(px(0.))
                .child(one_line(label.to_string(), FONT_MICRO, tokens.ink))
                .child(one_line(
                    match is_current {
                        true => "showing",
                        false => "",
                    },
                    FONT_CAPTION,
                    tokens.soft,
                )),
        )
        .child(actions)
        .into_any_element()
}

//! The window's shell: the app bar, the navigation rail, the scrolling content
//! area and the status bar.
//!
//! The shell owns the frame and nothing else. Each page's own body comes from
//! its module, so adding a page means adding a `Page` variant and a body — not
//! editing the frame.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};

use super::page::Page;
use super::stats;
use super::{
    activate, body, color, dot, icon, icon_label, muted, one_line, palette, panels, Tokens,
    APP_BAR_H, FONT_APP, FONT_CAPTION, FONT_MICRO, FONT_SUB, FONT_TITLE, PAGE_PAD, RADIUS_PILL,
    RADIUS_SM, RAIL_W, STATUS_BAR_H,
};
use crate::{ms_text, Controller, Link};
use gpui_kit::assets::IconName;
use ss550_tray::mark as art;

/// Builds the whole window: bar, rail, page and status line.
pub(crate) fn app_shell(controller: &mut Controller, cx: &mut Context<Controller>) -> Div {
    let tokens = Tokens::new(cx.theme());

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(tokens.bg)
        .text_color(tokens.ink)
        .child(app_bar(controller, tokens, cx))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_h(px(0.))
                .child(rail(controller, tokens, cx))
                .child(content(controller, tokens, cx)),
        )
        .child(status_bar(controller, tokens, cx))
}

/// The top row: what this window is, which deck it is following, and the three
/// controls that belong to the window rather than to a page.
fn app_bar(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let subtitle = deck_line(controller);

    div()
        .flex_none()
        .h(px(APP_BAR_H))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .px(px(16.))
        .bg(tokens.surface)
        .border_b_1()
        .border_color(tokens.border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .flex_1()
                .min_w(px(0.))
                .child(mark(tokens))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .min_w(px(0.))
                        .child(
                            div()
                                .text_size(px(FONT_APP))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Sh-Deck"),
                        )
                        .child(one_line(subtitle, FONT_CAPTION, tokens.soft)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .flex_none()
                .child(connection_pill(controller, tokens))
                .child(profile_button(controller, cx))
                .child(flavour_button(controller, tokens, cx)),
        )
}

/// The window's own mark: the deck's face, drawn from the same geometry the
/// `.ico` and the tray icon are rasterised from.
///
/// Drawn from elements, not loaded: an image asset would have to be fetched,
/// cached, scaled and re-exported for every window, and this is fifteen rounded
/// squares at one size. What matters is that the numbers come from
/// [`ss550_tray::mark`] — the program has one face, and a second copy of it here
/// is how a release ends up with two.
fn mark(tokens: Tokens) -> Div {
    /// The badge's side in the app bar, in pixels.
    const SIDE: f32 = 32.;

    let mut badge = div()
        .flex_none()
        .relative()
        .w(px(SIDE))
        .h(px(SIDE))
        .rounded(px(art::BADGE_RADIUS * SIDE))
        .bg(tokens.accent);

    for (index, (left, top, key)) in art::KEYS.iter().copied().enumerate() {
        let side = key * SIDE;
        let ink = match art::is_lit(index) {
            true => 1.,
            false => art::KEY_INK,
        };
        badge = badge.child(
            div()
                .absolute()
                .left(px(left * SIDE))
                .top(px(top * SIDE))
                .w(px(side))
                .h(px(side))
                .rounded(px(art::KEY_RADIUS * side))
                .bg(color(0xffffff).opacity(ink)),
        );
    }

    badge
}

/// The one line under the window's name: the deck's shape, or the reason there
/// is no shape to show.
fn deck_line(controller: &Controller) -> String {
    let Some(session) = controller.session.as_ref() else {
        return "No deck yet · Plug a deck in; this window follows whatever answers.".to_string();
    };

    let geometry = session.geometry();
    let spec = session.image_spec();
    let mut line = format!(
        "{} keys · {} columns · {}x{} images",
        geometry.key_count(),
        geometry.columns(),
        spec.width(),
        spec.height()
    );
    let dials = session.capabilities().dial_count();
    if dials > 0 {
        line.push_str(&format!(" · {dials} dials"));
    }
    line
}

/// Connection state, as a dot and a word.
///
/// Colour is never the only carrier: the word next to the dot says the same
/// thing for a reader who cannot see the difference between amber and red.
fn connection_pill(controller: &Controller, tokens: Tokens) -> Div {
    let tone = match controller.link {
        Link::Connected => tokens.good,
        Link::Connecting | Link::Waiting => tokens.warn,
        Link::Lost => tokens.bad,
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .h(px(32.))
        .px(px(12.))
        .rounded(px(RADIUS_PILL))
        .bg(tokens.elevated)
        .border_1()
        .border_color(tokens.border)
        .child(dot(tokens, tone, 8.))
        .child(
            div()
                .text_size(px(FONT_MICRO))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.soft)
                .child(controller.link.label()),
        )
}

/// The profile switcher: clicking it steps to the next profile, exactly as the
/// window has always done. The name is cut short because it comes from a file
/// the user owns and a control must not grow with it.
fn profile_button(controller: &mut Controller, cx: &mut Context<Controller>) -> Button {
    let active = controller.profile_store.active_name().unwrap_or("default");
    let label = format!("PROFILE · {}", super::shorten(active, 18));

    Button::new("profile-switcher")
        .secondary()
        .compact()
        .label(label)
        .tooltip("Step to the next profile")
        .on_click(cx.listener(|this: &mut Controller, _: &ClickEvent, _, cx| {
            // Cycle to the next profile on click - determine next profile first
            let next_name = {
                let profiles: Vec<String> = this.profile_store.profile_names();
                let current = this.profile_store.active_name().map(|s| s.to_string());
                let next = profiles
                    .iter()
                    .position(|p| Some(p.as_str()) == current.as_deref())
                    .map(|i| (i + 1) % profiles.len());
                next.and_then(|i| profiles.get(i).cloned())
            };

            // Now we can mutate since we've extracted all needed info
            if let Some(name) = next_name {
                this.switch_profile(&name);
                cx.notify();
            }
        }))
}

/// The flavour control: the palette the window is wearing, as a swatch and its
/// name.
///
/// It is a door, not a menu: the choice lives on the appearance card, where each
/// flavour can say what it looks like, and this is the shortcut that says which
/// one is in force from anywhere in the window. Clicking it opens that card.
fn flavour_button(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> AnyElement {
    let flavour = controller.flavour;
    let name = SharedString::from(super::shorten(flavour.name, 18));

    let control = div()
        .id("flavour-button")
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .h(px(32.))
        .px(px(10.))
        .rounded(px(RADIUS_SM))
        .bg(tokens.elevated)
        .border_1()
        .border_color(tokens.border)
        .hover(|style| style.bg(tokens.hover).border_color(tokens.accent))
        .child(super::swatch(tokens, flavour.accent, 12.))
        .child(
            div()
                .text_size(px(FONT_MICRO))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.soft)
                .child(name),
        );

    activate(control, cx, |this, _window, cx| {
        this.show_page(Page::Settings);
        cx.notify();
    })
    .into_any_element()
}

/// The navigation rail: three pages, the action palette, then what the window is
/// driving.
fn rail(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let current = controller.page;
    let items: Vec<AnyElement> = Page::ALL
        .iter()
        .copied()
        .map(|page| nav_item(page, page == current, tokens, cx))
        .collect();

    div()
        .flex_none()
        .flex()
        .flex_col()
        .w(px(RAIL_W))
        .h_full()
        .gap(px(4.))
        .px(px(12.))
        .pt(px(16.))
        // A little more at the bottom than at the top: the deck card is the
        // last thing before the status bar, and a card that touches its
        // neighbour reads as clipped even when it is not.
        .pb(px(20.))
        .bg(tokens.surface)
        .border_r_1()
        .border_color(tokens.border)
        .children(items)
        // The palette sits under the pages: the pages are where you go, the
        // palette is what you drag onto a key from anywhere. It takes the
        // height the deck card leaves, so its recents list scrolls inside the
        // rail instead of pushing the card off the bottom.
        .child(div().h(px(1.)).w_full().bg(tokens.border).my(px(8.)))
        .child(palette::actions_palette(controller, tokens))
        .child(rail_deck_card(controller, tokens, cx))
}

/// One rail row: a 36 px target with the page's icon, a radius, and a hover bed.
fn nav_item(page: Page, active: bool, tokens: Tokens, cx: &mut Context<Controller>) -> AnyElement {
    let tone = match active {
        true => tokens.accent_ink,
        false => tokens.soft,
    };

    let row = div()
        .id(SharedString::from(format!("nav-{}", page.label())))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .h(px(36.))
        .w_full()
        .px(px(12.))
        .rounded(px(RADIUS_SM))
        .text_size(px(FONT_SUB))
        .child(icon(page.icon(), 16., tone))
        .child(div().flex_1().min_w(px(0.)).truncate().child(page.label()));

    let row = if active {
        row.bg(tokens.accent_tint)
            .text_color(tokens.accent_ink)
            .font_weight(FontWeight::MEDIUM)
    } else {
        row.text_color(tokens.soft)
            .hover(|style| style.bg(tokens.hover).text_color(tokens.ink))
    };

    activate(row, cx, move |this, _window, cx| {
        this.show_page(page);
        cx.notify();
    })
    .into_any_element()
}

/// The two lines of the rail's deck card.
///
/// The first is the deck's own name. The second is the shape — but only when
/// the name is not already the shape: a deck that names itself by its geometry
/// would otherwise print the same numbers twice, one of them cut short.
pub(crate) fn deck_identity(label: &str, shape: &str) -> (String, Option<String>) {
    let detail = (label != shape).then(|| shape.to_string());
    (label.to_string(), detail)
}

/// The bottom of the rail: what the window is following, or the one control
/// that asks it to look again.
fn rail_deck_card(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let block = div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .min_w(px(0.))
        .p(px(12.))
        .rounded(px(RADIUS_SM))
        .bg(tokens.elevated)
        .border_1()
        .border_color(tokens.border);

    let Some(session) = controller.session.as_ref() else {
        return block
            .child(icon_label(IconName::HardDrive, "DECK", tokens))
            .child(body("No deck connected", tokens))
            .child(muted("Plug one in and this card fills itself in.", tokens))
            .child(
                Button::new("rail-reconnect")
                    .ghost()
                    .compact()
                    .icon(IconName::RefreshCw)
                    .label("RECONNECT")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.reconnect();
                        cx.notify();
                    })),
            );
    };

    let geometry = session.geometry();
    let spec = session.image_spec();
    let shape = format!(
        "{} keys · {} columns · {}x{} images",
        geometry.key_count(),
        geometry.columns(),
        spec.width(),
        spec.height()
    );
    let (identity, detail) = deck_identity(session.label(), &shape);

    let mut block = block
        .child(icon_label(IconName::HardDrive, "DECK", tokens))
        .child(
            // The deck's own name, allowed to wrap. This card has the bottom of the
            // rail to itself, and a name cut to "100x1…" says less than one extra
            // line does.
            div()
                .w_full()
                .min_w(px(0.))
                .text_size(px(FONT_SUB))
                .text_color(tokens.ink)
                .child(SharedString::from(identity)),
        );
    if let Some(detail) = detail {
        block = block.child(one_line(detail, FONT_CAPTION, tokens.soft));
    }
    block
}

/// The scrolling content area.
fn content(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let page = controller.page;
    let page_body = page_body(controller, tokens, page, cx);

    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(tokens.bg)
        .child(
            // The scroll belongs to this area and nowhere else, so a long page
            // scrolls under a fixed bar and a fixed rail instead of pushing
            // them off the window.
            div()
                .id("page-scroll")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scrollbar()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(PAGE_PAD))
                        .p(px(PAGE_PAD))
                        .w_full()
                        .child(page_header(page, tokens))
                        .child(page_body),
                ),
        )
}

/// The heading every page opens with: its icon, its name, and what it is for.
fn page_header(page: Page, tokens: Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .w_full()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .child(icon(page.icon(), 20., tokens.accent_ink))
                .child(
                    div()
                        .text_size(px(FONT_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.ink)
                        .child(page.title()),
                ),
        )
        .child(
            div()
                .text_size(px(FONT_SUB))
                .text_color(tokens.soft)
                .child(page.description()),
        )
}

/// The page's own body.
fn page_body(
    controller: &mut Controller,
    tokens: Tokens,
    page: Page,
    cx: &mut Context<Controller>,
) -> AnyElement {
    match page {
        Page::Deck => super::deck::deck_page(controller, tokens, cx).into_any_element(),
        Page::Actions => panels::actions_page(controller, tokens, cx).into_any_element(),
        Page::Stats => stats::stats_page(controller, tokens, cx).into_any_element(),
        Page::Settings => panels::settings_page(controller, tokens, cx).into_any_element(),
    }
}

/// The bottom row: what just happened, how fast the window is drawing, and the
/// quiet way into the log.
fn status_bar(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let fill = link_tone(controller.link, tokens);

    div()
        .flex_none()
        .h(px(STATUS_BAR_H))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .px(px(16.))
        .bg(tokens.surface)
        .border_t_1()
        .border_color(tokens.border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .flex_1()
                .min_w(px(0.))
                .child(dot(tokens, fill, 8.))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(FONT_MICRO))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.soft)
                        .child(controller.link.label()),
                )
                .child(one_line(controller.note.clone(), FONT_MICRO, tokens.soft)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .flex_none()
                .child(
                    div()
                        .text_size(px(FONT_MICRO))
                        .text_color(tokens.soft)
                        .child(format!("{} ms/frame", ms_text(controller.last_frame))),
                )
                .child(version_line(tokens, cx)),
        )
}

/// The version stamp. Five clicks on it open the diagnostic log.
///
/// It reads as a stamp, not as a control, so nothing invites a click: this is a
/// support tool, not a feature.
fn version_line(tokens: Tokens, cx: &mut Context<Controller>) -> AnyElement {
    div()
        .id("version-line")
        .cursor_pointer()
        .text_size(px(FONT_MICRO))
        .text_color(tokens.soft.opacity(0.7))
        .child(format!("v{}", env!("CARGO_PKG_VERSION")))
        .on_click(cx.listener(|this: &mut Controller, _: &ClickEvent, _, cx| {
            this.reveal_log();
            cx.notify();
        }))
        .into_any_element()
}

/// The colour a connection state is told apart by.
///
/// Colour never carries the state alone: the word beside the dot says the same
/// thing, for a reader who cannot tell amber from red.
fn link_tone(link: Link, tokens: Tokens) -> Hsla {
    match link {
        Link::Connected => tokens.good,
        Link::Connecting | Link::Waiting => tokens.warn,
        Link::Lost => tokens.bad,
    }
}

#[cfg(test)]
mod tests {
    use super::deck_identity;

    #[test]
    fn a_deck_that_names_itself_by_its_shape_says_it_once() {
        let shape = "12 keys · 4 columns · 96x96 images";
        // The session names an identity-less deck by its geometry, so the card
        // must not print the same numbers a second time.
        let (identity, detail) = deck_identity(shape, shape);

        assert_eq!(identity, shape);
        assert_eq!(detail, None, "the shape must not be repeated");
    }

    #[test]
    fn a_named_deck_gets_its_shape_underneath() {
        let shape = "12 keys · 4 columns · 96x96 images";
        let (identity, detail) = deck_identity("Studio Deck", shape);

        assert_eq!(identity, "Studio Deck");
        assert_eq!(detail.as_deref(), Some(shape));
    }
}

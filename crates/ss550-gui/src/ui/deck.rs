//! The `Deck` page: the key grid, the paint controls above it, and the
//! inspector for the key the window is looking at.
//!
//! Everything sized here comes from the session: the number of keys, the number
//! of columns, the rows and the capabilities. The page has no key count of its
//! own, so a deck of any shape draws itself correctly.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, RenderImage, SharedString, StatefulInteractiveElement as _,
    Styled as _,
};
use std::sync::Arc;

use super::{
    activate, body, card, color, editor, micro_label, muted, one_line, palette, Tokens, CARD_PAD,
    FONT_APP, FONT_CAPTION, FONT_MICRO, FONT_SUB, FONT_TITLE, RADIUS_MD, RADIUS_SM,
};
use crate::icons::{glyph_letter, Artwork};
use crate::titlestyle::{TitleLook as _, TitlePosition, TitleStyle};
use crate::{Controller, Paint, Slot};

/// The width a grid of keys may grow into before the tiles stop growing. Tiles
/// are square, so this is also the height they aim for.
const GRID_BUDGET: f32 = 600.;

/// The space between two tiles, and between two rows of them.
const GRID_GAP: f32 = 16.;

/// The smallest a tile may get: below this the key number and the action
/// summary stop being readable at arm's length.
const TILE_MIN: f32 = 104.;

/// The largest a tile may get, so a two-key deck does not draw two billboards.
const TILE_MAX: f32 = 160.;

/// The width of the column beside the grid: the inspector and the profile card.
const SIDE_W: f32 = 320.;

/// How wide one tile is on a deck with `columns` columns.
///
/// Square by construction, sized from the column count alone so every row has
/// the same width whatever it holds, clamped between a legible minimum and a
/// sane maximum. The grid is centred inside its card, so a narrow deck does not
/// leave its tiles hanging on the left.
pub(crate) fn tile_size(columns: usize) -> f32 {
    let columns = columns.max(1) as f32;
    (GRID_BUDGET / columns - GRID_GAP).clamp(TILE_MIN, TILE_MAX)
}

/// The narrowest the grid's card may be before its tiles would leave it.
///
/// Every row of that many tiles, the gaps between them, and the card's own
/// padding. The deck page uses it as the card's floor, and the row beside it
/// wraps instead of squeezing the grid out of the window.
pub(crate) fn grid_card_min(columns: usize) -> f32 {
    let columns = columns.max(1) as f32;
    columns * tile_size(columns as usize) + (columns - 1.) * GRID_GAP + 2. * CARD_PAD
}

/// What a tile draws: a flat colour, a picture, or an initial.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Face {
    /// A solid: white, red, or nothing at all.
    Swatch(Slot),
    /// The picture of what the key runs, or the one the profile put on it.
    Picture,
    /// No picture to draw: the initial of what the key runs, on the accent.
    Glyph,
}

/// What a key's tile asks for, before any picture is looked up.
///
/// The profile is the authority on a key that has an appearance; a key with none
/// shows what the deck shows, which is what the key runs — its program's icon or
/// its initial — unless the user painted it a colour, which is the explicit
/// override and wins.
pub(crate) fn face_for(
    appearance: Option<&ss550_engine::Appearance>,
    slot: Slot,
    artwork: Artwork,
) -> Face {
    match appearance {
        Some(ss550_engine::Appearance::Image { .. }) => Face::Picture,
        Some(ss550_engine::Appearance::Solid { rgb }) => Face::Swatch(crate::slot_for(*rgb)),
        None => match slot {
            // The colour row is the explicit override: a key the user painted
            // keeps that colour rather than the icon of what it runs.
            Slot::White | Slot::Red => Face::Swatch(slot),
            // The deck is showing the picture of what the key runs, so the tile
            // shows the same picture. A key that runs nothing has none, and
            // keeps the flat tile it had.
            Slot::Image => match artwork {
                Artwork::Icon => Face::Picture,
                Artwork::Glyph => Face::Glyph,
                Artwork::None => Face::Swatch(Slot::Image),
            },
            Slot::Off => Face::Swatch(Slot::Off),
        },
    }
}

/// What the tile really draws once the cache has answered.
///
/// A picture the cache cannot supply — the file was removed from the library
/// while the profile still names it, or the deck changed under the picture —
/// falls back to the initial and then to the flat colour. The tile is never
/// blank and the window never panics over a missing file.
pub(crate) fn resolved_face(
    appearance: Option<&ss550_engine::Appearance>,
    slot: Slot,
    artwork: Artwork,
    picture_ready: bool,
) -> Face {
    match face_for(appearance, slot, artwork) {
        Face::Picture if !picture_ready => match artwork {
            Artwork::Glyph => Face::Glyph,
            _ => Face::Swatch(slot),
        },
        face => face,
    }
}

/// What a key currently shows, in the words its tile prints.
pub(crate) fn appearance_label(slot: Slot) -> &'static str {
    match slot {
        Slot::White => "WHITE",
        Slot::Red => "RED",
        Slot::Image => "IMAGE",
        Slot::Off => "OFF",
    }
}

/// The two colours a tile can be painted in, and the ink that stands on them.
///
/// These are the deck's own key colours: data a tile displays, never chrome.
pub(crate) fn slot_colours(slot: Slot, tokens: Tokens) -> (Hsla, Hsla) {
    match slot {
        Slot::White => (color(0xe4e4e7), color(0x18181b)),
        Slot::Red => (color(0xdc2626), color(0xffffff)),
        Slot::Image => (color(0x3f3f46), color(0xfafafa)),
        Slot::Off => (tokens.elevated, tokens.soft),
    }
}

/// One key of the grid, already resolved from the controller.
struct Cell {
    /// The key's index in the deck's own order.
    index: usize,
    /// The binding number this key addresses, when the deck has that key.
    number: Option<ss550_engine::KeyNumber>,
    /// What the tile draws.
    face: Face,
    /// The picture the tile draws, when the face is a picture or an initial.
    picture: Option<Arc<RenderImage>>,
    /// The initial the tile draws, when there is no picture of it.
    glyph: Option<char>,
    /// The words the profile gives the key, when it gives any.
    title: Option<SharedString>,
    /// How those words are drawn, when the key has a style of its own.
    ///
    /// `None` is "the app's default", resolved when the row is built so a frame
    /// reads one value and not a chain of fallbacks.
    style: Option<TitleStyle>,
    pressed: bool,
    selected: bool,
    binding: Option<SharedString>,
}

/// The page body.
pub(crate) fn deck_page(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    if controller.session.is_none() {
        return empty_card(tokens, cx);
    }

    let columns = controller
        .session
        .as_ref()
        .map_or(1, |session| usize::from(session.geometry().columns()));

    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_start()
        .gap(px(24.))
        .w_full()
        // The grid keeps the width its tiles need; when the window is dragged
        // to the floor the column beside it wraps underneath instead of
        // squeezing the tiles out of the card.
        .child(key_card(controller, tokens, cx).min_w(px(grid_card_min(columns))))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .w(px(SIDE_W))
                .min_w(px(0.))
                .min_h(px(0.))
                // The column scrolls inside itself, not with the page: a user
                // reaching for the editor must not lose sight of the grid whose
                // key they are editing. The bound is what the window leaves once
                // the bars, the padding and the page's heading are taken out.
                .max_h(px(side_max_height(controller.viewport_h)))
                .overflow_y_scrollbar()
                .child(inspector_card(controller, tokens, cx))
                .child(profile_card(controller, tokens, cx))
                .child(super::pages::pages_card(controller, tokens, cx)),
        )
}

/// How tall the side column may get before it scrolls inside itself.
///
/// The window's own height minus everything around the scrollable part: the app
/// bar, the status bar, the page's padding and gap, and the heading. It is a
/// function of the window rather than a constant so a taller window shows more
/// of the column and a shorter one still leaves the grid visible.
pub(crate) fn side_max_height(viewport: f32) -> f32 {
    const CHROME: f32 = 220.;
    (viewport - CHROME).max(320.)
}

/// The card holding the toolbar, the grid and the deck-wide buttons.
fn key_card(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let keys = controller
        .session
        .as_ref()
        .map_or(0, |session| session.key_count());

    // How much of the deck is doing anything, which is the number a user opens
    // this page to see. The shape itself is already in the app bar.
    let bound = controller.session.as_ref().map_or(0, |session| {
        (0..session.key_count())
            .filter(|index| {
                crate::binding_number(session, *index)
                    .is_some_and(|number| controller.store.get(number).is_some())
            })
            .count()
    });

    card(tokens)
        .flex_1()
        .min_w(px(0.))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(micro_label("KEYS", tokens))
                .child(muted(format!("{bound} of {keys} bound"), tokens)),
        )
        .child(toolbar(controller, tokens, cx))
        .child(grid(controller, tokens, cx))
        .child(deck_actions(controller, tokens, cx))
}

/// The row above the grid: the deck-wide controls that act on every key.
///
/// There is no colour picker here on purpose. A key is painted from its own
/// card, on an explicit click on a swatch, because a key is something the user
/// inspects far more often than repaints.
fn toolbar(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let brightness = controller.brightness;
    let capabilities = controller
        .session
        .as_ref()
        .map(|session| session.capabilities());

    // A deck without a brightness control never sees one: the capability
    // decides, so nothing here can only be refused.
    let brightness_block = match capabilities {
        Some(caps) if caps.brightness() => brightness_row(brightness, tokens, cx),
        _ => div(),
    };

    let clear_all = match capabilities {
        Some(caps) if caps.clear_all() => Button::new("clear-all")
            .ghost()
            .label("CLEAR ALL")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.clear_all();
                cx.notify();
            }))
            .into_any_element(),
        _ => div().into_any_element(),
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .child(clear_all)
        .child(brightness_block)
}

/// The brightness steps, as chips that mark the one in force.
fn brightness_row(current: u8, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let mut chips = Vec::new();
    for value in [25_u8, 50, 75, 100] {
        let active = current == value;
        let chip = div()
            .id(("brightness", usize::from(value)))
            .flex()
            .items_center()
            .justify_center()
            .h(px(32.))
            .px(px(12.))
            .rounded(px(RADIUS_SM))
            .border_1()
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
            .child(format!("{value}%"));
        chips.push(activate(chip, cx, move |this, _window, cx| {
            this.set_brightness(value);
            cx.notify();
        }));
    }

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .child(micro_label("BRIGHTNESS", tokens))
        .children(chips)
}

/// A colour chip: a swatch of the colour and its name, ringed when in force.
///
/// Clicking one is an explicit instruction — it fills *the selected key*, not
/// whatever the next click lands on. Nothing in the window paints on a key
/// click any more: keys carry actions with parameters, and looking at one must
/// not change it.
fn swatch(paint: Paint, active: bool, tokens: Tokens, cx: &mut Context<Controller>) -> AnyElement {
    let (fill, ink, label) = match paint {
        Paint::Blank => (color(0x000000), color(0xffffff), "BLANK"),
        Paint::White => (color(0xe4e4e7), color(0x18181b), "WHITE"),
        Paint::Red => (color(0xdc2626), color(0xffffff), "RED"),
    };

    let chip = div()
        .id(SharedString::from(format!("swatch-{label}")))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        // The border is always two pixels wide: switching the ring on must not
        // move the chip, or the toolbar would twitch on every colour change.
        .border_2()
        .border_color(if active { tokens.accent } else { tokens.border })
        .rounded(px(RADIUS_SM))
        .bg(if active {
            tokens.elevated
        } else {
            tokens.surface
        })
        .h(px(36.))
        .pl(px(4.))
        .pr(px(12.))
        .child(
            div()
                .w(px(28.))
                .h(px(28.))
                .rounded(px(RADIUS_SM))
                .bg(fill)
                .border_1()
                .border_color(ink.opacity(0.25)),
        )
        .child(
            div()
                .text_size(px(FONT_MICRO))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if active { tokens.ink } else { tokens.soft })
                .child(label),
        )
        .hover(|style| style.bg(tokens.hover));

    activate(chip, cx, move |this, _window, cx| {
        this.paint = paint;
        let index = usize::from(this.selected_key.get().saturating_sub(1));
        this.paint_key(index);
        cx.notify();
    })
    .into_any_element()
}

/// The key grid itself, centred inside the card.
fn grid(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    // The whole grid is resolved first, so the controller is not borrowed while
    // the rows are being built. No picture is decoded here: the cache was
    // filled when the deck or the profile last changed.
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    // A drop lands on a key, and a key belongs to a deck: with no session there
    // is nothing to bind, so no tile accepts a palette entry.
    let accepts = controller.session.is_some();
    let columns = match controller.session.as_ref() {
        Some(session) => {
            // The page the deck is showing is the authority on what each key
            // looks like here: a folder may paint key 4 differently from the root.
            let keys = controller.current_keys();
            for row in session.rows() {
                let mut cells = Vec::new();
                for &index in row {
                    let number = crate::binding_number(session, index);
                    let action = number.and_then(|number| controller.store.get(number));
                    let binding = action.map(|action| SharedString::from(action.summary()));
                    let key = u16::try_from(index + 1).unwrap_or(u16::MAX);
                    let appearance = keys.and_then(|keys| keys.appearance_for(key));
                    // The picture comes from the library when the profile names
                    // one, and from the icon cache otherwise. Both were built
                    // when the deck, the profile or the binding changed, so this
                    // is a lookup and never a decode.
                    let picture = appearance
                        .and_then(|appearance| appearance.asset())
                        .and_then(|name| controller.previews.get(name))
                        .cloned()
                        .or_else(|| {
                            action
                                .and_then(|action| controller.icons.frame(action))
                                .cloned()
                        });
                    let artwork = controller.icons.artwork(action);
                    let slot = controller.slots.get(index).copied().unwrap_or(Slot::Off);
                    cells.push(Cell {
                        index,
                        number,
                        face: resolved_face(appearance, slot, artwork, picture.is_some()),
                        picture,
                        glyph: action.map(glyph_letter),
                        title: keys
                            .and_then(|keys| keys.title(key))
                            .map(SharedString::from),
                        style: keys.and_then(|keys| keys.title_style(key)).cloned(),
                        pressed: controller.pressed.get(index).copied().unwrap_or(false),
                        selected: number == Some(controller.selected_key),
                        binding,
                    });
                }
                rows.push(cells);
            }
            usize::from(session.geometry().columns())
        }
        None => 1,
    };

    let size = tile_size(columns);
    // The default style is copied out before the rows are built: a tile reads the
    // same value the deck's picture was composed with, and nothing in the frame
    // has to reach back into the controller for it.
    let default_style = controller.title_style.clone();
    // The deck's own tile side, which is what a pixel in the style means. A
    // window with no deck draws at its own scale.
    let device_side = controller
        .session
        .as_ref()
        .map_or(size, |session| f32::from(session.image_spec().height()));
    let built: Vec<Div> = rows
        .into_iter()
        .map(|cells| {
            let tiles: Vec<AnyElement> = cells
                .into_iter()
                .map(|cell| {
                    // A key with a style of its own wears it; the rest follow the
                    // default. `Cell` carries the key's override, resolved when
                    // the grid was built.
                    let style = cell.style.clone().unwrap_or_else(|| default_style.clone());
                    tile(cell, size, accepts, &style, device_side, tokens, cx)
                })
                .collect();
            div()
                .flex()
                .flex_row()
                .justify_center()
                .gap(px(GRID_GAP))
                .children(tiles)
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(GRID_GAP))
        .w_full()
        .py(px(8.))
        .children(built)
}

/// One square key: its number, what it shows, and what pressing it runs.
///
/// The tile is also the palette's drop target. `can_drop` is what makes the
/// "no deck" case honest: without a session there is no key to bind, so the
/// tile neither highlights nor accepts, and the palette says why.
fn tile(
    cell: Cell,
    size: f32,
    accepts: bool,
    style: &TitleStyle,
    device_side: f32,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> AnyElement {
    let index = cell.index;
    let target = cell.number;
    let binding = cell
        .binding
        .clone()
        .unwrap_or_else(|| SharedString::from("NO ACTION"));

    let mut tile = div()
        .id(("deck-key", index))
        .w(px(size))
        .h(px(size))
        .rounded(px(RADIUS_MD))
        .border_2()
        .bg(match cell.face {
            Face::Swatch(slot) => slot_colours(slot, tokens).0,
            Face::Picture => color(0x000000),
            Face::Glyph => tokens.accent,
        })
        .cursor_pointer();

    match cell.face {
        Face::Picture => {
            // The picture already carries the deck's letterboxing, so the tile
            // is a window onto it: the number sits on the picture, and the
            // bound action sits on a scrim along the bottom.
            let ring = if cell.pressed {
                tokens.accent
            } else if cell.selected {
                tokens.accent.opacity(0.55)
            } else {
                tokens.border
            };
            tile = tile
                .relative()
                .overflow_hidden()
                .border_color(ring)
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .w_full()
                        .h_full()
                        .child(
                            gpui_kit::img(
                                cell.picture.clone().expect("a picture face has a picture"),
                            )
                            .w_full()
                            .h_full(),
                        ),
                )
                .child(badge(format!("#{}", index + 1)))
                .child(scrim(cell.title.clone(), binding, size, style, device_side));
        }
        Face::Glyph => {
            // No icon to draw: the initial of what the key runs, on the accent
            // colour. The deck receives the same initial as a picture, so the
            // two never disagree about which letter a key is.
            let ring = if cell.pressed {
                tokens.accent
            } else if cell.selected {
                tokens.accent.opacity(0.55)
            } else {
                tokens.border
            };
            let letter = div()
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(size * 0.42))
                .font_weight(FontWeight::BOLD)
                .text_color(color(0xffffff))
                .child(SharedString::from(cell.glyph.unwrap_or('#').to_string()));

            tile = tile
                .relative()
                .overflow_hidden()
                .border_color(ring)
                .child(match cell.picture.clone() {
                    // The cache built the initial as a picture too; painting it
                    // is what keeps the tile and the deck the same image.
                    Some(picture) => div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .w_full()
                        .h_full()
                        .child(gpui_kit::img(picture).w_full().h_full()),
                    // A deck that has not answered yet has no picture size to
                    // build one for; the letter is drawn instead, so the tile is
                    // never empty.
                    None => div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .w_full()
                        .h_full()
                        .child(letter),
                })
                .child(badge(format!("#{}", index + 1)))
                .child(scrim(cell.title.clone(), binding, size, style, device_side));
        }
        Face::Swatch(slot) => {
            let (_, ink) = slot_colours(slot, tokens);
            // Pressed wins over selected, so a key held down is never ambiguous.
            let ring = if cell.pressed {
                tokens.accent
            } else if cell.selected {
                tokens.accent.opacity(0.55)
            } else {
                ink.opacity(0.16)
            };
            tile = tile
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .p(px(8.))
                .border_color(ring)
                .text_color(ink);

            // The lines of a flat-colour tile. The title is one of them, and the
            // position decides where: first, between the colour and what the key
            // runs, or last. A key with no title keeps exactly the three lines
            // it had, whatever the position says.
            let mut lines: Vec<AnyElement> = Vec::new();
            lines.push(
                div()
                    .text_size(px(FONT_SUB))
                    .font_weight(FontWeight::BOLD)
                    .child(format!("#{}", index + 1))
                    .into_any_element(),
            );
            lines.push(
                div()
                    .text_size(px(FONT_MICRO))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ink.opacity(0.7))
                    .child(appearance_label(slot))
                    .into_any_element(),
            );
            lines.push(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(FONT_MICRO))
                    .text_color(ink.opacity(0.75))
                    .child(binding)
                    .into_any_element(),
            );
            if let Some(title) = cell.title.clone() {
                let at = match style.position {
                    TitlePosition::Top => 0,
                    TitlePosition::Middle => 2,
                    // After the action, which is the last line pushed.
                    TitlePosition::Bottom => lines.len(),
                };
                let mut line = one_line(
                    title_words(&title, style),
                    style.window_font(size, device_side),
                    ink,
                )
                .w_full()
                .font_weight(FontWeight::SEMIBOLD);
                if let Some(family) = title_family(style) {
                    line = line.font_family(family);
                }
                lines.insert(at, line.into_any_element());
            }
            tile = tile.children(lines);
        }
    }

    tile.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
        // A click picks the key and nothing else. Touching a key to look at it
        // must never repaint it: keys carry actions with parameters, and a
        // gesture that also paints turns every inspection into a change.
        if let Some(session) = this.session.as_ref() {
            if let Some(number) = crate::binding_number(session, index) {
                this.select_key(number, window, cx);
            }
        }
        cx.notify();
    }))
    // The palette's half of the drop: `drag_over` is the look while a
    // compatible drag is over the tile, and it is only applied when `can_drop`
    // agrees, so a tile that cannot take the entry never pretends it can.
    .can_drop(move |_, _, _| accepts && target.is_some())
    .drag_over::<palette::PaletteDrag>(move |style, _, _, _| {
        style.bg(tokens.accent_tint).border_color(tokens.accent)
    })
    .on_drop(
        cx.listener(move |this, drag: &palette::PaletteDrag, window, cx| {
            let Some(number) = target else {
                return;
            };
            this.drop_palette_entry(number, drag, window, cx);
            cx.notify();
        }),
    )
    .into_any_element()
}

/// A small dark chip drawn over a picture: the key's number, in a corner.
fn badge(text: String) -> Div {
    div()
        .absolute()
        .top(px(8.))
        .left(px(8.))
        .px(px(6.))
        .py(px(2.))
        .rounded(px(RADIUS_SM))
        .bg(color(0x000000).opacity(0.6))
        .text_size(px(FONT_MICRO))
        .font_weight(FontWeight::BOLD)
        .text_color(color(0xffffff))
        .child(text)
}

/// The band over a picture tile: what the key says and runs.
///
/// The title goes above the action, in the same band: the two are one caption
/// on the picture, and a key with no title keeps exactly the one line it had.
/// The style's position decides which edge the band hugs — the same three
/// places the deck's own band is drawn in, from the same value — so the window
/// and the key on the desk never disagree about where a title sits.
fn scrim(
    title: Option<SharedString>,
    binding: SharedString,
    size: f32,
    style: &TitleStyle,
    device_side: f32,
) -> Div {
    let band = div()
        .w_full()
        .px(px(8.))
        .py(px(4.))
        .bg(color(0x000000).opacity(0.55));

    // The title's label is the style's size, computed from the style and the two
    // tiles alone: a frame never measures a string to decide how big it is.
    let title_size = style.window_font(size, device_side);
    let band = match title {
        Some(title) => {
            // The family the deck was drawn with, so the window and the key show
            // the same type and not only the same words.
            let words = title_words(&title, style);
            let mut caption =
                one_line(words, title_size, color(0xffffff)).font_weight(FontWeight::SEMIBOLD);
            if let Some(family) = title_family(style) {
                caption = caption.font_family(family);
            }
            band.flex()
                .flex_col()
                .gap(px(2.))
                .child(caption)
                .child(one_line(binding, FONT_MICRO, color(0xffffff)))
        }
        None => band.child(one_line(binding, FONT_MICRO, color(0xffffff))),
    };

    // A full-tile layer with the band at one end: the flex anchor moves the
    // band without anyone having to know how tall it turned out.
    let layer = div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w_full()
        .h_full()
        .flex()
        .flex_col();
    match style.position {
        TitlePosition::Top => layer.justify_start(),
        TitlePosition::Middle => layer.justify_center(),
        TitlePosition::Bottom => layer.justify_end(),
    }
    .child(band)
}

/// The family the window's title label is drawn in, when the style names one.
///
/// An empty name means "whatever sans the system defaults to", and that is the
/// window theme's own default already: leaving the family unset is how the
/// window and the deck agree on the system's font instead of on a name.
fn title_family(style: &TitleStyle) -> Option<SharedString> {
    (!style.family.is_empty()).then(|| SharedString::from(style.family.clone()))
}

/// The words a window label shows.
///
/// The deck's renderer applies the style's case rule to the string before it
/// shapes it ([`crate::titletext`]); the window's labels have to apply the same
/// rule, or a key under `UPPERCASE` would say two different things on the desk
/// and on the screen.
fn title_words(title: &SharedString, style: &TitleStyle) -> SharedString {
    match style.uppercase {
        true => SharedString::from(title.to_uppercase()),
        false => title.clone(),
    }
}

/// The buttons that act on the whole deck, below the grid.
fn deck_actions(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let capabilities = controller
        .session
        .as_ref()
        .map(|session| session.capabilities());

    let mut row = div().flex().flex_row().items_center().gap(px(8.));
    if let Some(caps) = capabilities {
        if caps.keep_alive() {
            row = row.child(
                Button::new("keepalive")
                    .ghost()
                    .label("KEEP ALIVE")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.heartbeat();
                        cx.notify();
                    })),
            );
        }
    }
    row = row
        .child(
            Button::new("reconnect")
                .ghost()
                .label("RECONNECT")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.reconnect();
                    cx.notify();
                })),
        )
        .child(
            Button::new("hide")
                .ghost()
                .label("HIDE · KEEPS RUNNING")
                .on_click(cx.listener(|this, _: &ClickEvent, _, _| {
                    this.hide_window();
                })),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .child(row)
        .child(one_line(
            "Closing the window leaves the controller running in the tray.",
            FONT_CAPTION,
            tokens.soft,
        ))
}

/// The card beside the grid: the key the window is looking at, the editor for
/// what it runs, and the picture it shows.
///
/// The editor is [`editor::key_editor`] and [`editor::picture_block`], the same
/// widgets the `Actions` page draws, so "edit this key" exists once. They are
/// here — on the page the key is on — because a drop lands on a key and must not
/// throw the user onto another page to fill in a form.
fn inspector_card(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let number = controller.selected_key;
    let index = usize::from(number.get().saturating_sub(1));
    let slot = controller.slots.get(index).copied().unwrap_or(Slot::Off);
    let bound = controller.store.get(number).cloned();
    let action = bound
        .as_ref()
        .map(|action| action.summary())
        .unwrap_or_else(|| "Nothing yet".to_string());
    let can_clear = controller
        .session
        .as_ref()
        .is_some_and(|session| session.capabilities().clear_key());

    // A key that shows a picture shows it here too: "SHOWS IMAGE" is the fact,
    // the picture is the confirmation the user actually wants. The picture is
    // the profile's when it names one, and the icon of what the key runs
    // otherwise — the same lookup the tile makes, so the two never disagree.
    let appearance = controller
        .current_keys()
        .and_then(|keys| keys.appearance_for(u16::from(number.get())));
    let artwork = controller.icons.artwork(bound.as_ref());
    let selected_picture = appearance
        .and_then(|appearance| appearance.asset())
        .and_then(|name| controller.previews.get(name))
        .cloned()
        .or_else(|| {
            bound
                .as_ref()
                .and_then(|action| controller.icons.frame(action))
                .cloned()
        });
    // The label follows what the tile draws, not what the key was last painted:
    // a key whose profile carries a picture shows the picture even when the
    // colour slot still holds the last solid it had.
    let shows = match resolved_face(appearance, slot, artwork, selected_picture.is_some()) {
        Face::Picture => "IMAGE",
        Face::Glyph => "INITIAL",
        Face::Swatch(_) => appearance_label(slot),
    };

    let badge = match selected_picture {
        Some(image) => div()
            .flex_none()
            .w(px(48.))
            .h(px(48.))
            .rounded(px(RADIUS_SM))
            .overflow_hidden()
            .bg(color(0x000000))
            .child(gpui_kit::img(image).w(px(48.)).h(px(48.)))
            .into_any_element(),
        None => div()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .w(px(48.))
            .h(px(48.))
            .rounded(px(RADIUS_SM))
            .bg(tokens.accent_tint)
            .text_size(px(FONT_TITLE))
            .font_weight(FontWeight::BOLD)
            .text_color(tokens.accent_ink)
            .child(format!("#{}", number.get()))
            .into_any_element(),
    };

    card(tokens)
        .w_full()
        .child(micro_label("SELECTED KEY", tokens))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .child(badge)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .min_w(px(0.))
                        .child(micro_label(format!("#{} SHOWS", number.get()), tokens))
                        .child(body(shows, tokens)),
                ),
        )
        .child(div().h(px(1.)).w_full().bg(tokens.border))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .min_w(px(0.))
                .child(micro_label("ACTION", tokens))
                .child(one_line(action, FONT_SUB, tokens.ink)),
        )
        .child(editor::key_editor(controller, tokens, cx))
        .child(div().h(px(1.)).w_full().bg(tokens.border))
        .child(
            // The colour row is the explicit override: a key painted here keeps
            // that colour instead of the icon of what it runs.
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(micro_label("COLOUR", tokens))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(swatch(Paint::Blank, slot == Slot::Off, tokens, cx))
                        .child(swatch(Paint::White, slot == Slot::White, tokens, cx))
                        .child(swatch(Paint::Red, slot == Slot::Red, tokens, cx))
                        .child(match can_clear {
                            true => Button::new("inspector-clear")
                                .ghost()
                                .label("BLANK")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    let index =
                                        usize::from(this.selected_key.get().saturating_sub(1));
                                    this.clear_painted_key(index);
                                    cx.notify();
                                }))
                                .into_any_element(),
                            false => div().into_any_element(),
                        }),
                ),
        )
        // Under the colour row: the picture the key shows. A picture wins over
        // the colour and the colour wins over what the key runs, so the card
        // states them from the action up: action, colour, picture.
        .child(editor::picture_block(controller, tokens, cx))
}

/// The deck's profiles, under the inspector.
///
/// The app bar steps to the next profile; this card lists them, so the window's
/// most-used switch is also the one you can aim at. Clicking a name switches to
/// it through the same call the app bar uses.
fn profile_card(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let active = controller.profile_store.active_name().map(str::to_string);
    let names = controller.profile_store.profile_names();

    let rows: Vec<AnyElement> = names
        .iter()
        .map(|name| {
            let is_active = active.as_deref() == Some(name.as_str());
            let row = div()
                .id(SharedString::from(format!("profile-row-{name}")))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .h(px(28.))
                .w_full()
                .min_w(px(0.))
                .px(px(8.))
                .rounded(px(RADIUS_SM))
                .border_1()
                .border_color(if is_active {
                    tokens.accent.opacity(0.5)
                } else {
                    tokens.border
                })
                .bg(if is_active {
                    tokens.accent_tint
                } else {
                    tokens.surface
                })
                .text_size(px(FONT_SUB))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(if is_active {
                            tokens.accent_ink
                        } else {
                            tokens.soft
                        })
                        .child(SharedString::from(name.clone())),
                )
                .child(match is_active {
                    true => div()
                        .flex_none()
                        .text_size(px(FONT_MICRO))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.accent_ink)
                        .child("ACTIVE")
                        .into_any_element(),
                    false => div().into_any_element(),
                })
                .hover(|style| style.bg(tokens.hover));

            let name = name.clone();
            activate(row, cx, move |this, _window, cx| {
                this.switch_profile(&name);
                cx.notify();
            })
            .into_any_element()
        })
        .collect();

    let count = names.len();
    let active_name = active.clone();
    card(tokens)
        .w_full()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(micro_label("PROFILE", tokens))
                .child(muted(format!("{count} saved"), tokens)),
        )
        .child(match rows.is_empty() {
            true => muted(
                "No profiles saved yet. Turn one on to keep a set of keys and bindings together.",
                tokens,
            ),
            false => div().flex().flex_col().gap(px(6.)).children(rows),
        })
        // Renaming is offered on the active profile: a key that switches to a
        // profile holds its name, so the engine moves the profile, the active
        // name and every key that points at it together.
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
                        .child(Input::new(&controller.profile_input)),
                )
                .child(
                    Button::new("rename-profile")
                        .ghost()
                        .label(match active_name {
                            Some(name) => format!("RENAME \"{name}\""),
                            None => "RENAME".to_string(),
                        })
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.rename_active_profile(window, cx);
                            cx.notify();
                        })),
                ),
        )
        .child(one_line(
            "Clicking a name switches to it; the field renames the one that is active.",
            FONT_CAPTION,
            tokens.soft,
        ))
}

/// What the page shows with no deck on the bus.
///
/// A framed placeholder sits where the grid will be, so the page reads as "this
/// fills in" rather than "this failed", and the only control is the one that
/// starts looking again.
fn empty_card(tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    card(tokens)
        .w_full()
        .child(micro_label("KEYS", tokens))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .w_full()
                .min_h(px(280.))
                .p(px(32.))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(tokens.border)
                .border_dashed()
                .bg(tokens.elevated)
                .child(
                    div()
                        .text_size(px(FONT_APP))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.ink)
                        .child("No deck connected"),
                )
                .child(
                    div()
                        .text_size(px(FONT_SUB))
                        .text_color(tokens.soft)
                        .text_align(gpui_kit::TextAlign::Center)
                        .child(
                            "Plug a deck in and this page fills itself in. The keys, the grid \
                             and the colour controls all come from whatever answers, and the \
                             window keeps looking on its own.",
                        ),
                )
                .child(
                    Button::new("deck-reconnect")
                        .primary()
                        .label("RECONNECT")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.reconnect();
                            cx.notify();
                        })),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::{
        appearance_label, face_for, grid_card_min, resolved_face, slot_colours, tile_size,
        title_words, Face, TILE_MAX, TILE_MIN,
    };
    use crate::icons::Artwork;
    use crate::titlestyle::TitleStyle;
    use crate::ui::{Tokens, PAGE_PAD, RAIL_W, WINDOW_MIN_W};
    use crate::Slot;
    use gpui_kit::component::Theme;
    use gpui_kit::SharedString;
    use ss550_engine::{Appearance, AssetName};

    #[test]
    fn the_window_label_says_what_the_key_says() {
        // The deck's renderer applies the style's case rule before it shapes the
        // words; a window label that skipped it would show `Canción` on a key
        // that says `CANCIÓN`, and the two would disagree about the words.
        let title = SharedString::from("Canción");
        let typed = TitleStyle::default();
        assert_eq!(title_words(&title, &typed), title);

        let loud = TitleStyle {
            uppercase: true,
            ..TitleStyle::default()
        };
        assert_eq!(title_words(&title, &loud), SharedString::from("CANCIÓN"));
    }

    #[test]
    fn a_tile_is_square_and_sized_from_the_column_count() {
        // Three columns is the common shape: the tile stops at its maximum and
        // the grid is centred, rather than the tiles growing without bound.
        assert_eq!(tile_size(3), TILE_MAX);
        assert_eq!(tile_size(4), 134.);
        assert_eq!(tile_size(5), TILE_MIN);
    }

    #[test]
    fn a_five_column_grid_fits_the_window_floor() {
        // The narrowest window the shell allows, minus the rail and the page's
        // padding: this is all the width a page ever gets.
        let content = WINDOW_MIN_W - RAIL_W - 2. * PAGE_PAD;
        assert!(
            grid_card_min(5) <= content,
            "five columns need {} px, the floor leaves {content} px",
            grid_card_min(5)
        );
        assert!(grid_card_min(4) <= content);
        // The floor is also wide enough for the column beside the grid to sit
        // there when a five-column deck is on screen.
        assert!(grid_card_min(5) + 24. + 320. > content, "the wrap is real");
    }

    #[test]
    fn a_wider_deck_never_draws_a_smaller_than_legible_tile() {
        for columns in 1..=12 {
            let size = tile_size(columns);
            assert!(
                (TILE_MIN..=TILE_MAX).contains(&size),
                "{columns} columns gave {size} px"
            );
        }
        // A deck that reports no columns at all is treated as one column: the
        // window must not divide by zero over a malformed shape.
        assert_eq!(tile_size(0), tile_size(1));
    }

    #[test]
    fn every_appearance_has_its_own_label() {
        assert_eq!(appearance_label(Slot::White), "WHITE");
        assert_eq!(appearance_label(Slot::Red), "RED");
        assert_eq!(appearance_label(Slot::Image), "IMAGE");
        assert_eq!(appearance_label(Slot::Off), "OFF");
    }

    #[test]
    fn a_painted_key_keeps_its_colour_apart_from_the_chrome() {
        // The two paint colours are data: they must differ from every chrome
        // colour the tiles sit on, or a white key would vanish into a card.
        let tokens = Tokens::new(&Theme::default());
        let (white, _) = slot_colours(Slot::White, tokens);
        let (red, _) = slot_colours(Slot::Red, tokens);
        let (off, _) = slot_colours(Slot::Off, tokens);

        assert_ne!(white, tokens.surface);
        assert_ne!(red, tokens.accent);
        assert_ne!(off, white);
        assert_ne!(off, red);
    }

    #[test]
    fn a_profile_picture_asks_the_tile_for_the_picture() {
        let picture = Appearance::image(AssetName::parse("cover.png").expect("a plain name"));

        assert_eq!(
            face_for(Some(&picture), Slot::Off, Artwork::None),
            Face::Picture
        );
        // A picture wins over whatever colour the key was last painted: the
        // profile is the authority on a key that has an appearance.
        assert_eq!(
            face_for(Some(&picture), Slot::Red, Artwork::Glyph),
            Face::Picture
        );
    }

    #[test]
    fn a_key_without_a_picture_keeps_its_flat_colour() {
        let solid = Appearance::solid([0xDC, 0x26, 0x26]);

        // A solid is drawn as the swatch that matches it, not as the slot the
        // window happens to be holding.
        assert_eq!(
            face_for(Some(&solid), Slot::White, Artwork::None),
            Face::Swatch(Slot::Red)
        );
        assert_eq!(
            face_for(None, Slot::White, Artwork::Icon),
            Face::Swatch(Slot::White)
        );
        assert_eq!(
            face_for(None, Slot::Off, Artwork::None),
            Face::Swatch(Slot::Off)
        );
    }

    #[test]
    fn a_key_shows_what_it_runs_when_the_user_has_not_chosen_a_colour() {
        // The deck is showing the picture of what the key runs, so the tile
        // shows the same one: the program's icon, or its initial when the icon
        // could not be read.
        assert_eq!(
            face_for(None, Slot::Image, Artwork::Icon),
            Face::Picture,
            "a program's icon"
        );
        assert_eq!(
            face_for(None, Slot::Image, Artwork::Glyph),
            Face::Glyph,
            "the initial of what the key runs"
        );
        // A key that runs nothing has no picture to show and keeps its tile.
        assert_eq!(
            face_for(None, Slot::Image, Artwork::None),
            Face::Swatch(Slot::Image)
        );
    }

    #[test]
    fn the_colour_row_wins_over_the_icon() {
        // The colour row is the explicit override: a key painted white or red
        // keeps that colour, whatever it runs.
        for slot in [Slot::White, Slot::Red] {
            for artwork in [Artwork::Icon, Artwork::Glyph, Artwork::None] {
                assert_eq!(
                    face_for(None, slot, artwork),
                    Face::Swatch(slot),
                    "{slot:?} must survive {artwork:?}"
                );
            }
        }
    }

    #[test]
    fn a_picture_that_is_not_cached_falls_back_to_the_flat_tile() {
        // The profile still names a picture the library no longer has: the tile
        // is painted with its plain colour rather than staying blank.
        let picture = Appearance::image(AssetName::parse("gone.png").expect("a plain name"));

        assert_eq!(
            resolved_face(Some(&picture), Slot::Off, Artwork::None, false),
            Face::Swatch(Slot::Off)
        );
        assert_eq!(
            resolved_face(Some(&picture), Slot::Off, Artwork::None, true),
            Face::Picture
        );
        // A solid never consults the picture cache at all.
        assert_eq!(
            resolved_face(
                Some(&Appearance::solid([0xFF, 0xFF, 0xFF])),
                Slot::Red,
                Artwork::None,
                false
            ),
            Face::Swatch(Slot::White)
        );
    }

    #[test]
    fn an_icon_that_is_not_cached_falls_back_to_the_initial() {
        // A key that runs a program is never blank, even when the picture of it
        // is not in the cache: the initial is drawn instead.
        assert_eq!(
            resolved_face(None, Slot::Image, Artwork::Glyph, false),
            Face::Glyph
        );
        assert_eq!(
            resolved_face(None, Slot::Image, Artwork::Icon, false),
            Face::Swatch(Slot::Image),
            "an icon with no picture and no letter to draw keeps the tile"
        );
    }
}

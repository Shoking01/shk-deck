//! The one widget that edits a key.
//!
//! The `Deck` page's inspector and the `Actions` page both draw this, so there
//! is exactly one implementation of "edit this key": the three kinds, the field
//! each kind needs, SAVE BINDING, CLEAR KEY, and — under the colour row — the
//! picture the key shows. A second copy is how the two places end up
//! disagreeing about what a field means.
//!
//! A palette template dropped on a key opens this editor where the key is — on
//! the page the key is on, with the first field focused. The drop never moves
//! the window and never writes: the key keeps what it had until SAVE BINDING.
//!
//! The picture belongs to the key, not to a page of its own: it is chosen with
//! the system's own file dialog, imported into the folder the app owns, and put
//! on the key from the same click. Nothing is decoded while a frame is drawn —
//! the thumbnail the block paints is the one the key's tile is fed from.

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
    activate, micro_label, muted, one_line, path_line, segment, titlecontrols, Tokens,
    FONT_CAPTION, FONT_SUB, RADIUS_SM,
};
use crate::apps;
use crate::{Controller, DraftKind, NO_DECK_PICTURE};

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
        ))
        .child(segment(
            SharedString::from("draft-plugin"),
            "LIVE STAT",
            draft == DraftKind::Plugin,
            tokens,
            cx,
            |this, _window, cx| {
                this.draft = DraftKind::Plugin;
                this.note = "Pick a measurement, then press SAVE BINDING.".into();
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
        DraftKind::Plugin => plugin_fields(controller, tokens, cx),
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
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.clear_binding(window, cx);
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

    // The title sits under the action and above the colour row the caller adds:
    // it is what the key *says*, which is a property of the key and not of what
    // it runs. It saves on its own, because naming a key is not rebinding it.
    let title_row = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .child(
            Field::new()
                .label("TITLE · the words this key shows, drawn on its tile")
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
                                .child(Input::new(&controller.title_input)),
                        )
                        .child(
                            Button::new("save-title")
                                .ghost()
                                .label("SAVE TITLE")
                                .disabled(!has_deck)
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.save_title(cx);
                                    cx.notify();
                                })),
                        ),
                ),
        )
        .child(match has_deck {
            // The cap is the engine's, not a second copy of the rule: the field
            // says what will be accepted, and the engine is what refuses.
            true => one_line(
                format!(
                    "Up to {} characters. Leave it empty to take the title off the key.",
                    ss550_engine::MAX_TITLE_CHARS
                ),
                FONT_CAPTION,
                tokens.soft,
            ),
            // The same sentence the refusal uses, so the caption and the note
            // cannot disagree about why the button does nothing.
            false => one_line(crate::NO_DECK_TITLE, FONT_CAPTION, tokens.soft),
        });

    // The key's own style, under the words it draws. Offered only when the key
    // says something: a style with no words has nothing to draw, and the engine
    // clears both together anyway.
    let key = u16::from(controller.selected_key.get());
    let has_title = controller
        .current_keys()
        .and_then(|keys| keys.title(key))
        .is_some();
    let style_block = match has_title {
        true => {
            let target = crate::FontTarget::Key(key);
            let mut block = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .w_full()
                .child(micro_label("THIS KEY'S STYLE", tokens))
                .child(titlecontrols::rows(controller, target, tokens, cx));
            if let Some(reset) = titlecontrols::reset(controller, target, tokens, cx) {
                block = block.child(reset);
            }
            Some(block.child(muted(
                "Keys without a style of their own follow TITLE DEFAULTS on the Settings page.",
                tokens,
            )))
        }
        false => None,
    };

    let mut form = div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .w_full()
        .child(kind)
        .child(fields)
        .child(save_row)
        .child(one_line(saved_to, FONT_CAPTION, tokens.soft))
        .child(title_row);
    if let Some(block) = style_block {
        form = form.child(block);
    }
    form
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

/// The measurement a key shows, chosen from what the plugins of this build
/// publish.
///
/// The list is the catalogue itself, not a copy: a cell this build does not have
/// cannot be chosen, and the same list is what refuses a binding that names one
/// after a plugin goes away. Nothing is measured to draw it — the plugin states
/// its cells once, when it is built.
fn plugin_fields(controller: &mut Controller, tokens: Tokens, cx: &mut Context<Controller>) -> Div {
    let chosen = controller.draft_plugin.clone();
    let cells: Vec<ss550_plugin_api::CellSpec> = controller.plugins.cells().to_vec();

    let mut list = div().flex().flex_col().gap(px(2.)).w_full();
    for (position, cell) in cells.iter().enumerate() {
        let picked = chosen.as_ref().is_some_and(|(plugin, id)| {
            plugin == cell.plugin().as_str() && id == cell.cell().as_str()
        });
        let plugin = cell.plugin().to_string();
        let id = cell.cell().to_string();
        let swatch = cell.accent();
        let row = div()
            .id(SharedString::from(format!("plugin-cell-{position}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(px(34.))
            .w_full()
            .min_w(px(0.))
            .px(px(8.))
            .rounded(px(RADIUS_SM))
            .bg(if picked {
                tokens.accent_tint
            } else {
                tokens.elevated
            })
            .child(
                div()
                    .flex_none()
                    .w(px(10.))
                    .h(px(10.))
                    .rounded(px(5.))
                    .bg(gpui_kit::rgb(
                        (u32::from(swatch[0]) << 16)
                            | (u32::from(swatch[1]) << 8)
                            | u32::from(swatch[2]),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(one_line(cell.label(), FONT_SUB, tokens.ink))
                    .child(one_line(cell.hint(), FONT_CAPTION, tokens.soft)),
            );
        list = list.child(activate(row, cx, move |this, _window, cx| {
            this.draft = DraftKind::Plugin;
            this.draft_plugin = Some((plugin.clone(), id.clone()));
            this.note = format!(
                "{} will show {} — press SAVE BINDING.",
                this.selected_key.get(),
                id
            )
            .into();
            cx.notify();
        }));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .child(micro_label("MEASUREMENT · what this key shows", tokens))
        .child(list)
        .child(one_line(
            "The key redraws itself as the measurement moves. Pressing it refreshes it.",
            FONT_CAPTION,
            tokens.soft,
        ))
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

/// The picture the selected key shows.
///
/// The picture belongs to the key: this is the same editor the action is set
/// in, under the colour row, and not a page of its own. CHOOSE A PICTURE opens
/// the system's own file dialog, and the click that comes back imports the file
/// into the library the window owns and puts it on the key — in that order, so a
/// key never names a picture the library does not hold.
///
/// Nothing here reads the library or decodes a picture while a frame is drawn:
/// the thumbnail is the one the tile is fed from, and the folder the pictures
/// live in is a path the controller already has.
pub(crate) fn picture_block(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let has_deck = controller.session.is_some();
    let key = u16::from(controller.selected_key.get());
    let in_use = controller
        .current_keys()
        .and_then(|keys| keys.appearance_for(key))
        .and_then(|appearance| appearance.asset())
        .cloned();
    let folder = controller.assets.root().display().to_string();

    // The key's own picture, from the cache the tile is fed from. A picture the
    // profile names but no cache holds — no deck to size it, or a file that is
    // gone — leaves the box empty rather than lying about what the key shows.
    let picture = in_use
        .as_ref()
        .and_then(|name| controller.previews.get(name))
        .cloned();

    let head = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(micro_label("PICTURE", tokens))
        .child(muted(
            match in_use.is_some() {
                true => "On this key",
                false => "None on this key",
            },
            tokens,
        ));

    // A key with no picture says so in the box as well as in the line beside it:
    // a dashed outline is the window's own "nothing here" shape.
    let face = match in_use.is_some() {
        true => thumbnail(picture, tokens),
        false => div()
            .flex_none()
            .w(px(44.))
            .h(px(44.))
            .rounded(px(6.))
            .border_1()
            .border_dashed()
            .border_color(tokens.border)
            .bg(tokens.elevated),
    };

    // The picker is offered whether or not the key already shows something: a
    // key with no picture needs it to get one, and a key with one needs it to
    // get a different one. It is disabled, not hidden, when there is no deck —
    // a picture is fitted to the deck's tile, so there is nothing to fit yet,
    // and the caption under the buttons says exactly that.
    let controls = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
        .child(
            Button::new("choose-picture")
                .primary()
                .label("CHOOSE A PICTURE")
                .disabled(!has_deck)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.choose_picture();
                    cx.notify();
                })),
        )
        // Quiet on purpose: taking a picture off a key is the smaller of the two
        // choices, and a second loud button next to the first invites the click
        // nobody meant.
        .child(
            Button::new("remove-picture")
                .ghost()
                .label("REMOVE PICTURE")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.remove_artwork();
                    cx.notify();
                })),
        );

    let state = match &in_use {
        Some(name) => name.to_string(),
        None => "This key shows no picture.".to_string(),
    };

    let mono = cx.theme().mono_font_family.clone();

    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .w_full()
        .child(head)
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(12.))
                .w_full()
                .min_w(px(0.))
                .child(face)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .flex_1()
                        .min_w(px(0.))
                        .child(one_line(state, FONT_SUB, tokens.ink))
                        .child(controls),
                ),
        )
        // The folder and the button that opens it on one row: where the
        // pictures are kept, and the way to look at them, are one thought.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .w_full()
                .min_w(px(0.))
                .child(
                    Button::new("open-picture-folder")
                        .ghost()
                        .label("OPEN FOLDER")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.open_assets_folder();
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(path_line(folder, tokens, &mono)),
                ),
        )
        .child(match has_deck {
            true => one_line(
                format!(
                    "Choosing a picture copies it into that folder, puts it on key #{key} and \
                     repaints the key."
                ),
                FONT_CAPTION,
                tokens.soft,
            ),
            // The same sentence the refusal uses, so the caption and the note
            // cannot disagree about why the button does nothing.
            false => one_line(NO_DECK_PICTURE, FONT_CAPTION, tokens.soft),
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_picture_controls_never_decode_a_picture() {
        // The block paints the thumbnail the tile is fed from, and the folder is
        // a path it already has. A decoder or a folder listing reachable from
        // this module would run inside `render`, once per picture per frame,
        // which is exactly what the caches and the click exist to prevent. The
        // needles are spelled in pieces so this file does not match itself.
        let source = include_str!("editor.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        let forbidden = [
            ["preview", "_frame"].concat(),
            ["assets", ".read"].concat(),
            ["assets", ".list"].concat(),
            ["image", "::open"].concat(),
            ["library", "_previews"].concat(),
        ];

        for needle in forbidden {
            assert!(
                !production.contains(needle.as_str()),
                "the picture controls do their own work ({needle}) instead of reading what they \
                 were given"
            );
        }
    }

    #[test]
    fn the_picture_controls_ask_the_system_for_the_file() {
        // The one way a picture gets onto a key: the platform's dialog, then the
        // library's import, then the assignment. The click is in `main`, so this
        // is a statement about the shape of the block: it offers the picker and
        // no list to browse.
        let source = include_str!("editor.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);

        assert!(
            production.contains(["choose", "_picture"].concat().as_str()),
            "the block offers the picker"
        );
        assert!(
            !production.contains(["picture", "_rows"].concat().as_str()),
            "there is no library list left to build rows for"
        );
    }
}

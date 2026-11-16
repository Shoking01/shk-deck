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
//! The picture belongs to the key, not to a page of its own: the library is a
//! folder the user drops files into, and the list of it opens here. Nothing is
//! decoded while a frame is drawn — the fitted thumbnails are built when the
//! list is opened and when the folder is re-read.

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::Field;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _,
};

use super::panels::{placeholder, thumbnail};
use super::{
    activate, micro_label, muted, one_line, path_line, segment, Tokens, FONT_CAPTION, FONT_MICRO,
    FONT_SUB, RADIUS_SM,
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

/// How tall the list of pictures is before it scrolls.
///
/// Tall enough for four rows. The library is a folder the user drops files
/// into, so a longer one scrolls instead of pushing the form out of the card.
const PICTURE_LIST_H: f32 = 232.;

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

/// The picture the selected key shows, and the library it comes from.
///
/// The picture belongs to the key: this is the same editor the action is set
/// in, under the colour row, and not a page of its own. The list is the folder
/// the user drops files into, so it is a toggle rather than a second window —
/// choosing a row assigns the picture and nothing else changes.
///
/// Every thumbnail is a lookup in the cache that was filled when the list was
/// opened or the folder was re-read; nothing here decodes a picture.
pub(crate) fn picture_block(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let has_deck = controller.session.is_some();
    let key = u16::from(controller.selected_key.get());
    let in_use = controller
        .profile_store
        .active()
        .and_then(|profile| profile.appearance_for(key))
        .and_then(|appearance| appearance.asset())
        .cloned();
    let folder = controller.assets.root().display().to_string();
    let count = controller.library.len();
    let empty = count == 0;
    let open = controller.picture_list_open;

    // The key's own picture, from the cache the tile is fed from. A picture the
    // profile names but no cache holds — no deck to size it, or a file that is
    // gone — leaves the box empty rather than lying about what the key shows.
    let picture = in_use.as_ref().and_then(|name| {
        controller
            .previews
            .get(name)
            .or_else(|| controller.library_previews.get(name))
            .cloned()
    });

    let head = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(micro_label("PICTURE", tokens))
        .child(muted(
            format!("{count} picture{}", if count == 1 { "" } else { "s" }),
            tokens,
        ));

    // Rows are resolved before a widget is built: a row's click handler needs
    // the entity context, and the library is read while the rows are drawn. A
    // closed list builds none of them — a list nobody is looking at is work a
    // frame must not do.
    let mut rows: Vec<AnyElement> = Vec::new();
    if open {
        for row in picture_rows(&controller.library, in_use.as_ref()) {
            let thumb = controller.library_previews.get(&row.name).cloned();
            let name = row.name.clone();
            let element = div()
                .id(SharedString::from(format!("picture-row-{}", row.name)))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.))
                .w_full()
                .min_w(px(0.))
                .p(px(8.))
                .rounded(px(RADIUS_SM))
                .border_1()
                .border_color(if row.in_use {
                    tokens.accent.opacity(0.6)
                } else {
                    tokens.border
                })
                .bg(if row.in_use {
                    tokens.accent_tint
                } else {
                    tokens.elevated
                })
                .child(thumbnail(thumb, tokens))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .flex_1()
                        .min_w(px(0.))
                        .child(one_line(
                            SharedString::from(row.name.to_string()),
                            FONT_SUB,
                            tokens.ink,
                        ))
                        .child(one_line(row.size, FONT_CAPTION, tokens.soft)),
                )
                .child(match row.in_use {
                    true => div()
                        .flex_none()
                        .text_size(px(FONT_MICRO))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.accent_ink)
                        .child("IN USE")
                        .into_any_element(),
                    false => div().into_any_element(),
                })
                .hover(|style| style.bg(tokens.hover));

            rows.push(
                activate(element, cx, move |this, _window, cx| {
                    this.assign_asset(name.clone());
                    cx.notify();
                })
                .into_any_element(),
            );
        }
    }

    let list: AnyElement = match rows.is_empty() {
        true => placeholder(
            "No pictures yet",
            &format!(
                "Drop a picture into {folder}, then press REFRESH. Nothing is stretched: it is \
                 fitted to the deck's tile, and re-encoded smaller if it would not fit."
            ),
            tokens,
        )
        .into_any_element(),
        false => div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w_full()
            .max_h(px(PICTURE_LIST_H))
            .overflow_y_scrollbar()
            .children(rows)
            .into_any_element(),
    };

    let face = match in_use.is_some() {
        true => thumbnail(picture, tokens),
        // A key with no picture says so in the box as well as in the line beside
        // it: a dashed outline is the window's own "nothing here" shape.
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

    let mut controls = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(8.));
    // Nothing to choose from is stated by the list below, not by a button that
    // would open onto the same sentence.
    if !empty {
        controls = controls.child(
            Button::new("toggle-picture-list")
                .ghost()
                .label(match open {
                    true => "HIDE THE PICTURES",
                    false => "CHOOSE A PICTURE",
                })
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.toggle_picture_list();
                    cx.notify();
                })),
        );
    }
    controls = controls.child(
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

    let mut block = div()
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
        );

    // An empty folder is worth saying whether or not the list was opened: it is
    // the reason CHOOSE A PICTURE is not there.
    if empty || open {
        block = block.child(list);
    }

    block
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(
                    Button::new("refresh-pictures")
                        .ghost()
                        .label("REFRESH")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.reload_library();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("open-picture-folder")
                        .ghost()
                        .label("OPEN FOLDER")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.open_assets_folder();
                            cx.notify();
                        })),
                ),
        )
        .child(match has_deck {
            true => one_line(
                format!("Choosing a picture puts it on key #{key} and repaints the key."),
                FONT_CAPTION,
                tokens.soft,
            ),
            // The same sentence the refusal uses, so the caption and the note
            // cannot disagree about why nothing happened.
            false => one_line(NO_DECK_PICTURE, FONT_CAPTION, tokens.soft),
        })
        .child(path_line(folder, tokens, &cx.theme().mono_font_family))
}

/// One row of the picture list, resolved from the library before a widget is
/// built.
struct PictureRow {
    /// The picture's name inside the library.
    name: ss550_engine::AssetName,
    /// Its size on disk, as the row states it.
    size: String,
    /// Whether the selected key already shows it.
    in_use: bool,
}

/// Every picture in the library, with the one the key shows marked.
///
/// The library is the folder the user drops files into, so the list is exactly
/// what the folder holds: nothing is filtered, and nothing is read from the disk
/// here. The mark is decided by name, which is how a profile names its picture.
fn picture_rows(
    library: &[ss550_engine::Asset],
    in_use: Option<&ss550_engine::AssetName>,
) -> Vec<PictureRow> {
    library
        .iter()
        .map(|asset| PictureRow {
            name: asset.name.clone(),
            size: size_label(asset.bytes),
            in_use: in_use == Some(&asset.name),
        })
        .collect()
}

/// A picture's size, in the unit the list states it in.
fn size_label(bytes: u64) -> String {
    // Rounded up: a file of a few hundred bytes is a size, not "0 KB".
    format!("{} KB", bytes.div_ceil(1024))
}

#[cfg(test)]
mod tests {
    use super::{picture_rows, size_label};
    use ss550_engine::{Asset, AssetName};

    fn asset(name: &str, bytes: u64) -> Asset {
        Asset {
            name: AssetName::parse(name).expect("a library name"),
            bytes,
        }
    }

    #[test]
    fn every_picture_in_the_library_gets_a_row() {
        let library = [asset("one.png", 2048), asset("two.jpg", 4096)];

        let rows = picture_rows(&library, None);

        assert_eq!(rows.len(), library.len());
        assert_eq!(rows[0].name, library[0].name);
        assert_eq!(rows[1].name, library[1].name);
    }

    #[test]
    fn the_picture_the_key_shows_is_the_only_one_marked() {
        let library = [asset("one.png", 2048), asset("two.jpg", 4096)];
        let in_use = AssetName::parse("two.jpg").expect("a library name");

        let rows = picture_rows(&library, Some(&in_use));

        let marked: Vec<&str> = rows
            .iter()
            .filter(|row| row.in_use)
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(marked, ["two.jpg"], "exactly the key's picture is marked");
    }

    #[test]
    fn a_row_states_the_size_of_the_file() {
        assert_eq!(size_label(2048), "2 KB");
        assert_eq!(size_label(1536), "2 KB");
        // A file smaller than a kilobyte still reads as a size, not as zero.
        assert_eq!(size_label(512), "1 KB");
    }

    #[test]
    fn an_empty_library_has_no_rows_so_the_block_says_so() {
        // The block draws its empty state from an empty list: a row appearing
        // for a folder with nothing in it would hide that sentence.
        let named = AssetName::parse("one.png").expect("a library name");
        assert!(picture_rows(&[], None).is_empty());
        assert!(picture_rows(&[], Some(&named)).is_empty());
    }

    #[test]
    fn the_picture_controls_never_decode_a_picture() {
        // The list paints what the cache holds: the fitted frames are built when
        // the list is opened and when the folder is re-read. A decoder reachable
        // from this module would run inside `render`, once per picture per frame,
        // which is exactly what the caches exist to prevent. The needles are
        // spelled in pieces so this file does not match itself.
        let source = include_str!("editor.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        let forbidden = [
            ["preview", "_frame"].concat(),
            ["assets", ".read"].concat(),
            ["image", "::open"].concat(),
        ];

        for needle in forbidden {
            assert!(
                !production.contains(needle.as_str()),
                "the picture controls decode a picture ({needle}) instead of reading the cache"
            );
        }
    }
}

//! The `Actions`, `Artwork` and `Settings` pages.
//!
//! Each page is a stack of cards in the same language the `Deck` page uses:
//! a micro-labelled title, a short line of secondary text, and controls that
//! are only offered when they can act. Nothing here decodes a picture while a
//! frame is being drawn: the gallery asks the controller to fit the library when
//! the page opens, and paints what the cache already holds.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::Field;
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Theme, ThemeMode};
use gpui_kit::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _,
};
use std::sync::Arc;

use super::{
    activate, body, card, color, micro_label, muted, one_line, path_line, segment, Tokens,
    FONT_CAPTION, FONT_MICRO, FONT_SUB, RADIUS_MD, RADIUS_SM,
};
use crate::{ms_text, Controller, DraftKind};

/// The width of the column that holds the big preview on the Artwork page.
const PREVIEW_W: f32 = 320.;

// ── Actions ─────────────────────────────────────────────────────────────

/// What each key runs when it is pressed on the deck.
pub(crate) fn actions_page(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let has_deck = controller.session.is_some();
    let selected = controller.selected_key;
    let launch_active = controller.draft == DraftKind::Launch;

    let picker = key_picker(controller, tokens, cx);
    let picker_card = card(tokens).w_full().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(micro_label("KEYS", tokens))
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

    let kind = div()
        .flex()
        .flex_row()
        .gap(px(8.))
        .child(segment(
            SharedString::from("draft-launch"),
            "LAUNCH APP",
            launch_active,
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
            !launch_active,
            tokens,
            cx,
            |this, _window, cx| {
                this.draft = DraftKind::OpenUrl;
                this.note = "Fill the URL, then press SAVE BINDING.".into();
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

    let editor_body = match launch_active {
        true => launch_fields,
        false => url_fields,
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

    let editor_card = card(tokens)
        .w_full()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(micro_label("WHAT THIS KEY RUNS", tokens))
                .child(muted(format!("#{}", selected.get()), tokens)),
        )
        .child(kind)
        .child(editor_body)
        .child(save_row)
        .child(one_line(saved_to, FONT_CAPTION, tokens.soft));

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
                .label("REMOVE")
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.remove_binding(key);
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
                .child(micro_label("BOUND RIGHT NOW", tokens))
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

// ── Artwork ─────────────────────────────────────────────────────────────

/// The pictures a key can show, and what one looks like on the deck.
pub(crate) fn artwork_page(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let has_deck = controller.session.is_some();
    let has_selection = controller.selected_asset.is_some();
    let selected = controller.selected_key;
    let folder = controller.assets.root().display().to_string();
    let count = controller.library.len();

    // The gallery: one row per picture, with the fitted tile as its thumbnail.
    let mut rows: Vec<AnyElement> = Vec::new();
    for (position, asset) in controller.library.iter().enumerate() {
        let is_selected = controller.selected_asset.as_ref() == Some(&asset.name);
        let thumb = controller.previews.get(&asset.name).cloned();
        let name = asset.name.clone();

        let row = div()
            .id(SharedString::from(format!("asset-row-{position}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .w_full()
            .min_w(px(0.))
            .p(px(8.))
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(if is_selected {
                tokens.accent.opacity(0.6)
            } else {
                tokens.border
            })
            .bg(if is_selected {
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
                        SharedString::from(asset.name.to_string()),
                        FONT_SUB,
                        tokens.ink,
                    ))
                    .child(one_line(
                        format!("{:.0} KB", asset.bytes as f64 / 1024.0),
                        FONT_CAPTION,
                        tokens.soft,
                    )),
            )
            .child(match is_selected {
                true => div()
                    .flex_none()
                    .text_size(px(FONT_MICRO))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.accent_ink)
                    .child("SELECTED")
                    .into_any_element(),
                false => div().into_any_element(),
            })
            .hover(|style| style.bg(tokens.hover));

        rows.push(
            activate(row, cx, move |this, _window, cx| {
                this.select_asset(name.clone());
                cx.notify();
            })
            .into_any_element(),
        );
    }

    let library_body = match rows.is_empty() {
        true => placeholder(
            "No pictures yet",
            &format!(
                "Drop a picture into {folder}, then press REFRESH. Nothing is stretched: it is \
                 fitted to the deck's tile, and re-encoded smaller if it would not fit."
            ),
            tokens,
        ),
        false => div().flex().flex_col().gap(px(8.)).children(rows),
    };

    let library = card(tokens)
        .flex_1()
        .min_w(px(280.))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(micro_label("PICTURES", tokens))
                .child(muted(
                    format!("{count} picture{}", if count == 1 { "" } else { "s" }),
                    tokens,
                )),
        )
        .child(library_body);

    // The big tile: the picker's own preview when there is one, and otherwise
    // whatever the cache already holds for the highlighted picture — so a
    // selection made before this page was opened still shows its picture.
    let large: AnyElement = match &controller.preview {
        Some((name, image)) => big_preview(name.as_str(), image.clone(), tokens),
        None => match controller.selected_asset.as_ref().and_then(|name| {
            controller
                .previews
                .get(name)
                .map(|image| (name.clone(), image.clone()))
        }) {
            Some((name, image)) => big_preview(name.as_str(), image, tokens),
            None => placeholder(
                "Nothing picked",
                "Pick a picture from the gallery to see what the deck will show, black bands \
                 and all.",
                tokens,
            )
            .into_any_element(),
        },
    };

    let preview = card(tokens)
        .w(px(PREVIEW_W))
        .min_w(px(0.))
        .child(micro_label("PREVIEW", tokens))
        .child(large)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .w_full()
                .child(
                    Button::new("use-asset")
                        .primary()
                        .label(format!("USE ON KEY #{}", selected.get()))
                        .disabled(!has_deck || !has_selection)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.assign_asset();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("remove-artwork")
                        .secondary()
                        .label("REMOVE ARTWORK")
                        .disabled(!has_deck)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.remove_artwork();
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            Button::new("refresh-assets")
                                .ghost()
                                .label("REFRESH")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    match this.refresh_library() {
                                        Ok(count) => {
                                            this.note = format!(
                                                "{count} picture{} in the library.",
                                                if count == 1 { "" } else { "s" }
                                            )
                                            .into();
                                        }
                                        Err(error) => {
                                            this.note = format!(
                                                "Could not read the picture folder: {error}"
                                            )
                                            .into();
                                        }
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("open-assets")
                                .ghost()
                                .label("OPEN FOLDER")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.open_assets_folder();
                                    cx.notify();
                                })),
                        ),
                ),
        )
        .child(match has_deck {
            true => one_line(
                format!("The picture is assigned to key #{}.", selected.get()),
                FONT_CAPTION,
                tokens.soft,
            ),
            false => one_line(
                "Connect a deck to put a picture on a key: the tile size comes from it.",
                FONT_CAPTION,
                tokens.soft,
            ),
        })
        .child(one_line(folder, FONT_CAPTION, tokens.soft));

    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_start()
        .gap(px(24.))
        .w_full()
        .child(library)
        .child(preview)
}

/// A gallery thumbnail: the fitted tile, or a quiet box when it is not ready.
fn thumbnail(picture: Option<Arc<gpui_kit::RenderImage>>, tokens: Tokens) -> Div {
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

/// The fitted tile at full size, with the name under it.
fn big_preview(name: &str, image: Arc<gpui_kit::RenderImage>, tokens: Tokens) -> AnyElement {
    let side = 168.;
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .items_center()
        .w_full()
        .child(
            div()
                .w(px(side))
                .h(px(side))
                .rounded(px(RADIUS_SM))
                .overflow_hidden()
                .bg(color(0x000000))
                .child(gpui_kit::img(image).w(px(side)).h(px(side))),
        )
        .child(one_line(
            format!("{name} · fitted to the deck's tile"),
            FONT_CAPTION,
            tokens.soft,
        ))
        .into_any_element()
}

// ── Settings ────────────────────────────────────────────────────────────

/// How the controller behaves when the window is closed, and where it keeps
/// what it knows.
pub(crate) fn settings_page(
    controller: &mut Controller,
    tokens: Tokens,
    cx: &mut Context<Controller>,
) -> Div {
    let is_dark = cx.theme().is_dark();
    let mono = cx.theme().mono_font_family.clone();
    let tray_note = match &controller.tray_error {
        Some(error) => format!("The tray icon is missing: {error}"),
        None => "The tray icon is in the notification area.".to_string(),
    };

    // ── Startup ──
    let mut startup = card(tokens)
        .w_full()
        .child(micro_label("STARTUP", tokens))
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
    let appearance = card(tokens)
        .w_full()
        .child(micro_label("APPEARANCE", tokens))
        .child(muted(
            "The window follows the theme at once. Keys keep the colours they show: those are \
             the deck's, not the window's.",
            tokens,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(segment(
                    SharedString::from("theme-dark"),
                    "DARK",
                    is_dark,
                    tokens,
                    cx,
                    |_this, window, cx| {
                        Theme::change(ThemeMode::Dark, Some(window), cx);
                        cx.notify();
                    },
                ))
                .child(segment(
                    SharedString::from("theme-light"),
                    "LIGHT",
                    !is_dark,
                    tokens,
                    cx,
                    |_this, window, cx| {
                        Theme::change(ThemeMode::Light, Some(window), cx);
                        cx.notify();
                    },
                )),
        );

    // ── Files ──
    let bindings = match &controller.store_path {
        Some(path) => path.display().to_string(),
        None => "Saved per deck, once one answers".to_string(),
    };
    let files = card(tokens)
        .w_full()
        .child(micro_label("FILES", tokens))
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
        .child(one_line(
            "Paths are cut short from the left, so the name at the end stays visible.",
            FONT_CAPTION,
            tokens.soft,
        ));

    // ── About ──
    let about = card(tokens)
        .w_full()
        .child(micro_label("ABOUT", tokens))
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
        .child(files)
        .child(about)
}

// ── Shared pieces for the three pages ───────────────────────────────────

/// A framed explanation, used wherever a list would be empty.
fn placeholder(title: &'static str, text: &str, tokens: Tokens) -> Div {
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

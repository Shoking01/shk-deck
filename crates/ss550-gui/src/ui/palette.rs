//! The action palette: the draggable entries at the foot of the rail.
//!
//! Two sections, and only two. `ACTIONS` is what a key can be made to run: the
//! three templates, then one row per plugin this build ships (`Clock`,
//! `System stats`), because a person drags the plugin and then picks which of its
//! cells the key shows. `RECENTS` is what this deck was last given, newest first.
//! The rail does **not** mirror every bound action: the `Actions` page already
//! lists those under `BOUND RIGHT NOW`, and a second copy is noise.
//!
//! A palette entry is a promise, not a write. Dragging a *template* — `Run app`,
//! `Open URL`, `Keystrokes` — onto a key only opens that key's editor on that
//! kind, where the key is; dragging a *plugin* opens the same editor with the
//! cell list narrowed to that plugin's cells; dragging a finished action — one
//! from `RECENTS` — copies it, because it has nothing left to ask for. None of
//! these paths touches the store from here: all end in the save the editor
//! already uses, and a template or a plugin only ends there when the user presses
//! SAVE BINDING.
//!
//! This module owns what can be *dragged*; the drop target is the key tile in
//! [`super::deck`], because that is where a key is. The two halves meet on
//! [`PaletteDrag`], and [`drop_plan`] is the whole of what a drop may do — a
//! function over plain data, so "a template never writes" is a test and not a
//! convention.

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    div, px, AnyElement, AppContext as _, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window,
};
use ss550_engine::{Action, Recents};
use ss550_plugins::Catalogue;

use super::{icon, icon_label, one_line, Tokens, FONT_CAPTION, FONT_MICRO, FONT_SUB, RADIUS_SM};
use crate::{Controller, DraftKind};

/// What `RECENTS` says when this deck has not been given anything yet.
///
/// One line and nothing else: an empty state that explains itself does not need
/// a button, and the templates directly above are the thing it is explaining.
pub(crate) const EMPTY_RECENTS: &str = "Actions you add show up here";

/// What a palette entry offers when it is dragged onto a key.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PaletteDrag {
    /// A kind of action that still needs its parameters.
    Template(DraftKind),
    /// One plugin, by id: the drop opens the reading editor narrowed to its
    /// cells. A plugin is one row, whatever it measures.
    Plugin(String),
    /// An action that is already complete.
    Action(Action),
}

/// What dropping a palette entry on a key means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DropPlan {
    /// Open the editor on this kind. The key keeps whatever it had, and
    /// nothing is saved until SAVE BINDING.
    ///
    /// `plugin` is the plugin a plugin row was dragged from: the editor lists
    /// that plugin's cells and no others. A template carries none, because it
    /// is not asking about a plugin.
    OpenEditor {
        /// Which kind of action the editor opens on.
        kind: DraftKind,
        /// The plugin the list is narrowed to, when a plugin row was dragged.
        plugin: Option<String>,
    },
    /// Assign this finished action, through the same save the editor uses.
    Assign(Action),
}

/// Reads a drag as the one thing a drop may do.
///
/// A template can never come out of here as an assignment: that is the whole
/// difference between "make this key able to run an app" and "make this key run
/// this app". A plugin row is the same promise one step deeper: it decides
/// *which cells are offered*, never which cell is bound.
pub(crate) fn drop_plan(drag: &PaletteDrag) -> DropPlan {
    match drag {
        PaletteDrag::Template(kind) => DropPlan::OpenEditor {
            kind: *kind,
            plugin: None,
        },
        PaletteDrag::Plugin(plugin) => DropPlan::OpenEditor {
            kind: DraftKind::Plugin,
            plugin: Some(plugin.clone()),
        },
        PaletteDrag::Action(action) => DropPlan::Assign(action.clone()),
    }
}

/// The templates, in the order the rail lists them.
///
/// A reading is deliberately absent: `LIVE STAT` is not one row but one row per
/// plugin, and those come from the catalogue so this build cannot offer a
/// plugin it does not ship.
pub(crate) const TEMPLATES: [DraftKind; 3] =
    [DraftKind::Launch, DraftKind::OpenUrl, DraftKind::Keys];

/// What a template's row reads.
///
/// `DraftKind::Plugin` keeps its name here because the kind still exists — it is
/// what `LIVE STAT` opens — but its row is a plugin's own name, not this one.
pub(crate) fn template_label(kind: DraftKind) -> &'static str {
    match kind {
        DraftKind::Launch => "Run app",
        DraftKind::OpenUrl => "Open URL",
        DraftKind::Keys => "Keystrokes",
        DraftKind::Plugin => "Live stat",
        DraftKind::Media => "Media key",
    }
}

/// One row of the palette.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    /// What the row reads.
    pub(crate) label: SharedString,
    /// What dragging the row offers.
    pub(crate) drag: PaletteDrag,
}

/// Every entry the `ACTIONS` section lists: the templates, then one row per
/// plugin, in the order the catalogue holds them.
///
/// The plugin rows are read from the catalogue rather than listed here, so a
/// plugin this build ships is a row without anyone remembering to add one, and a
/// plugin it does not ship cannot be offered.
///
/// Bound actions are deliberately absent. They live on the `Actions` page under
/// `BOUND RIGHT NOW`; repeating them here made the rail a second, stale copy of
/// a list that already has a home.
pub(crate) fn entries(catalogue: &Catalogue) -> Vec<Entry> {
    let templates = TEMPLATES.iter().map(|kind| Entry {
        label: SharedString::from(template_label(*kind)),
        drag: PaletteDrag::Template(*kind),
    });
    let plugins = catalogue.groups().iter().map(|group| Entry {
        label: SharedString::from(group.name()),
        drag: PaletteDrag::Plugin(group.id().to_string()),
    });
    templates.chain(plugins).collect()
}

/// The rows the `RECENTS` section lists: one per recorded action, newest first.
///
/// The engine owns the cap and the de-duplication, so this is a projection of
/// [`Recents::entries`], not a second policy: the rail cannot show more rows
/// than the list holds, and it cannot invent an order of its own.
pub(crate) fn recents_entries(recents: &Recents) -> Vec<Entry> {
    recents
        .entries()
        .iter()
        .map(|action| Entry {
            label: SharedString::from(action.summary()),
            drag: PaletteDrag::Action(action.clone()),
        })
        .collect()
}

/// The `ACTIONS` and `RECENTS` blocks at the foot of the rail.
///
/// `ACTIONS` grows with the plugins this build ships — one row each — and is
/// still the shorter half. `RECENTS` takes whatever height is left and scrolls
/// inside itself when eight rows do not fit, so the page links above and the deck
/// card below never move.
pub(crate) fn actions_palette(controller: &Controller, tokens: Tokens) -> Div {
    // A drop lands on a key, and a key belongs to a deck. Without one the rows
    // stay on screen — the palette is still readable, and the window is still
    // navigable — but they are not drag sources, and the line underneath says
    // why.
    let droppable = controller.session.is_some();

    let template_rows: Vec<AnyElement> = entries(&controller.plugins)
        .into_iter()
        .enumerate()
        .map(|(position, entry)| row("palette", position, entry, droppable, tokens))
        .collect();

    let recent_rows: Vec<AnyElement> = recents_entries(&controller.recents)
        .into_iter()
        .enumerate()
        .map(|(position, entry)| row("recent", position, entry, droppable, tokens))
        .collect();

    let mut actions = div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(6.))
        .w_full()
        .min_w(px(0.))
        .child(icon_label(IconName::Plus, "ACTIONS", tokens));

    if !droppable {
        actions = actions.child(
            div()
                .text_size(px(FONT_CAPTION))
                .text_color(tokens.soft)
                .child("Plug a deck in to drop these on a key."),
        );
    }

    actions = actions.child(
        div()
            .id("palette-list")
            .flex()
            .flex_col()
            .gap(px(2.))
            .w_full()
            .min_w(px(0.))
            .children(template_rows),
    );

    let mut recents = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .gap(px(6.))
        .w_full()
        .min_w(px(0.))
        .child(icon_label(IconName::RotateCw, "RECENTS", tokens));

    if recent_rows.is_empty() {
        recents = recents.child(
            div()
                .text_size(px(FONT_CAPTION))
                .text_color(tokens.soft)
                .child(EMPTY_RECENTS),
        );
    } else {
        recents = recents.child(
            div()
                .id("recents-list")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .gap(px(2.))
                .w_full()
                .min_w(px(0.))
                .overflow_y_scrollbar()
                .children(recent_rows),
        );
    }

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .gap(px(12.))
        .w_full()
        .min_w(px(0.))
        .child(actions)
        .child(recents)
}

/// One draggable row.
///
/// The drag payload carries the entry itself, so the drop target never has to
/// look anything up: what the pointer holds is exactly what the drop acts on.
/// `namespace` keeps a template row and a recent row from sharing an element
/// id, because both lists start at position zero.
fn row(
    namespace: &'static str,
    position: usize,
    entry: Entry,
    droppable: bool,
    tokens: Tokens,
) -> AnyElement {
    // A bound action already exists; a template and a plugin both *add* one, so
    // they share the marker. The label a plugin row rides with is its own name,
    // which the payload alone cannot spell — hence the copy taken here.
    let adds = !matches!(entry.drag, PaletteDrag::Action(_));
    let ghost_label = entry.label.clone();
    let glyph = entry_icon(&entry.drag);

    let row = div()
        .id(SharedString::from(format!("{namespace}-{position}")))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .h(px(30.))
        .w_full()
        .min_w(px(0.))
        .px(px(8.))
        .rounded(px(RADIUS_SM))
        .text_size(px(FONT_SUB))
        .child(marker(
            glyph,
            if adds { tokens.accent_ink } else { tokens.soft },
        ))
        .child(one_line(entry.label, FONT_SUB, tokens.ink));

    if !droppable {
        // Dimmed, not hidden: the row is still a fact about the app, and the
        // caption above it says what is missing.
        return row.opacity(0.5).into_any_element();
    }

    row.cursor_grab()
        .hover(|style| style.bg(tokens.hover))
        // `on_drag` is what makes the row a drag source: gpui starts the drag
        // on mouse-down and follows the pointer, and the constructor below is
        // the little card that rides along. The card is built from the row, so
        // a row and the thing it drags can never disagree.
        .on_drag(entry.drag, move |drag, _, _, cx| {
            let label = match drag {
                PaletteDrag::Template(kind) => SharedString::from(template_label(*kind)),
                PaletteDrag::Plugin(_) => ghost_label.clone(),
                PaletteDrag::Action(action) => SharedString::from(action.summary()),
            };
            cx.new(|_| DragGhost { label, tokens })
        })
        .into_any_element()
}

/// The little mark in front of a row: what the entry would do, in one icon.
///
/// A template or a plugin *adds* something to a key, so it wears the accent; a
/// recent action already exists, so it wears the secondary ink. The colour is
/// the only difference — the icon says what kind of thing the row is, which is
/// what makes a column of eight rows scannable.
fn marker(glyph: IconName, ink: Hsla) -> Div {
    div().flex_none().child(icon(glyph, 14., ink))
}

/// The icon a row wears, from what dragging it would offer.
fn entry_icon(drag: &PaletteDrag) -> IconName {
    match drag {
        PaletteDrag::Template(kind) => template_icon(*kind),
        PaletteDrag::Plugin(plugin) => plugin_icon(plugin),
        PaletteDrag::Action(action) => action_icon(action),
    }
}

/// The icon a template wears: the thing it makes the key able to do.
///
/// Every name here is in `gpui_kit`'s bundled set; the enum lists all of Lucide,
/// but an icon the asset source does not carry paints nothing at all.
fn template_icon(kind: DraftKind) -> IconName {
    match kind {
        DraftKind::Launch => IconName::SquareTerminal,
        DraftKind::OpenUrl => IconName::Globe,
        DraftKind::Keys => IconName::CaseSensitive,
        DraftKind::Plugin => IconName::ChartPie,
        DraftKind::Media => IconName::Play,
    }
}

/// The icon a plugin's own row wears.
///
/// A plugin this build ships is named here; one it does not is a row from a
/// catalogue that grew, and the generic reading mark is the honest fallback
/// rather than a guess at what the plugin measures.
fn plugin_icon(id: &str) -> IconName {
    match id {
        "system-stats" => IconName::Cpu,
        "clock" => IconName::Calendar,
        "media" => IconName::Play,
        "fps" => IconName::ChartPie,
        _ => IconName::ChartPie,
    }
}

/// The icon a finished action wears, by what it does when pressed.
fn action_icon(action: &Action) -> IconName {
    match action {
        Action::Launch { .. } => IconName::SquareTerminal,
        Action::OpenUrl { .. } => IconName::Globe,
        Action::Keys { .. } => IconName::CaseSensitive,
        Action::Media { .. } => IconName::Play,
        Action::Folder { .. } => IconName::FolderOpen,
        Action::Back => IconName::ArrowLeft,
        Action::Profile { .. } => IconName::User,
        Action::Plugin { .. } => IconName::ChartPie,
    }
}

/// What follows the pointer while a palette entry is dragged.
///
/// It is a view of its own because gpui asks for one: the drag image is a
/// rendered element, not a string. Nothing here decodes or measures anything,
/// so a drag costs one tiny frame.
pub(crate) struct DragGhost {
    label: SharedString,
    tokens: Tokens,
}

impl DragGhost {
    /// The card that rides the pointer while something is dragged.
    ///
    /// Built by the drag source itself, so a page that drags its own things
    /// still rides the one ghost the window has.
    pub(crate) fn new(label: SharedString, tokens: Tokens) -> Self {
        Self { label, tokens }
    }
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(6.))
            .rounded(px(RADIUS_SM))
            .bg(self.tokens.elevated)
            .border_1()
            .border_color(self.tokens.accent)
            .text_size(px(FONT_MICRO))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(self.tokens.ink)
            .child(self.label.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        drop_plan, entries, recents_entries, template_label, DropPlan, PaletteDrag, EMPTY_RECENTS,
        TEMPLATES,
    };
    use crate::DraftKind;
    use ss550_engine::{Action, Recents, MAX_RECENTS};
    use ss550_plugins::Catalogue;

    fn launch(program: &str) -> Action {
        Action::Launch {
            program: program.into(),
            args: Vec::new(),
        }
    }

    #[test]
    fn the_actions_section_offers_the_templates_and_one_row_per_plugin() {
        let catalogue = Catalogue::built_in();
        let listed = entries(&catalogue);

        assert_eq!(listed.len(), TEMPLATES.len() + catalogue.groups().len());

        let labels: Vec<&str> = listed.iter().map(|entry| entry.label.as_ref()).collect();
        assert_eq!(
            &labels[..TEMPLATES.len()],
            ["Run app", "Open URL", "Keystrokes"]
        );

        for (entry, kind) in listed.iter().take(TEMPLATES.len()).zip(TEMPLATES) {
            assert!(matches!(entry.drag, PaletteDrag::Template(listed) if listed == kind));
        }

        // One row per plugin, named the way the plugin names itself: a person
        // drags "Clock", not "the seconds of the clock", and not nine cells.
        for (entry, group) in listed[TEMPLATES.len()..].iter().zip(catalogue.groups()) {
            assert_eq!(entry.label.as_ref(), group.name());
            assert_eq!(entry.drag, PaletteDrag::Plugin(group.id().to_string()));
        }
    }

    #[test]
    fn a_plugin_row_opens_the_editor_on_that_plugin() {
        let drag = PaletteDrag::Plugin(String::from("clock"));

        assert_eq!(
            drop_plan(&drag),
            DropPlan::OpenEditor {
                kind: DraftKind::Plugin,
                plugin: Some(String::from("clock")),
            },
            "a plugin row narrows the list; it never binds a cell by itself"
        );
    }

    #[test]
    fn an_empty_recents_list_has_no_rows_and_says_what_it_is_for() {
        let listed = recents_entries(&Recents::new());
        assert!(listed.is_empty());
        assert!(!EMPTY_RECENTS.trim().is_empty());
        assert!(
            EMPTY_RECENTS.contains("show up here"),
            "the empty state must say what the section is for"
        );
    }

    #[test]
    fn the_recents_section_lists_what_was_recorded_newest_first() {
        let mut recents = Recents::new();
        let first = launch("C:\\Apps\\brave.exe");
        let second = Action::keys("ctrl+shift+m").expect("a valid combination");
        recents.record(&first);
        recents.record(&second);

        let listed = recents_entries(&recents);

        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].label.as_ref(), "Ctrl+Shift+M");
        assert_eq!(listed[1].label.as_ref(), "LAUNCH brave.exe");
        assert_eq!(listed[0].drag, PaletteDrag::Action(second));
        assert_eq!(listed[1].drag, PaletteDrag::Action(first));
    }

    #[test]
    fn the_recents_section_never_lists_more_than_the_engine_keeps() {
        let mut recents = Recents::new();
        for index in 0..=MAX_RECENTS {
            recents.record(&launch(&format!("C:\\Apps\\app{index}.exe")));
        }

        assert_eq!(recents_entries(&recents).len(), MAX_RECENTS);
    }

    #[test]
    fn a_template_drop_is_an_intent_and_a_recent_drop_is_a_write() {
        for kind in TEMPLATES {
            assert_eq!(
                drop_plan(&PaletteDrag::Template(kind)),
                DropPlan::OpenEditor { kind, plugin: None },
                "a template must never assign"
            );
        }

        let action = launch("C:\\Apps\\brave.exe");
        assert_eq!(
            drop_plan(&PaletteDrag::Action(action.clone())),
            DropPlan::Assign(action),
            "a finished action has nothing left to ask"
        );
    }

    #[test]
    fn every_kind_of_action_is_reachable_from_the_palette() {
        // `DraftKind` and the palette are two lists of the same kinds; a kind
        // with no row would be a kind the user cannot reach. Three kinds are
        // templates; the reading is one row per plugin, taken from the
        // catalogue, so this build cannot show a rail with no reading in it.
        assert_eq!(TEMPLATES.len(), 3);
        for kind in [DraftKind::Launch, DraftKind::OpenUrl, DraftKind::Keys] {
            assert!(TEMPLATES.contains(&kind), "{kind:?} has no row");
            assert!(!template_label(kind).trim().is_empty());
        }
        assert!(
            entries(&Catalogue::built_in())
                .iter()
                .any(|entry| matches!(entry.drag, PaletteDrag::Plugin(_))),
            "no plugin row: a reading could not be dragged onto a key"
        );
    }
}

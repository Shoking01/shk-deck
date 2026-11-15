//! The action palette: the draggable entries at the foot of the rail.
//!
//! A palette entry is a promise, not a write. Dragging a *template* — `Run app`,
//! `Open URL`, `Keystrokes` — onto a key only opens that key's editor on that
//! kind; dragging an action that is already bound copies it, because it has
//! nothing left to ask for. Neither path touches the store from here: both end
//! in the save the editor already uses, and a template only ends there when the
//! user presses SAVE BINDING.
//!
//! This module owns what can be *dragged*; the drop target is the key tile in
//! [`super::deck`], because that is where a key is. The two halves meet on
//! [`PaletteDrag`], and [`drop_plan`] is the whole of what a drop may do — a
//! function over plain data, so "a template never writes" is a test and not a
//! convention.

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    div, px, AnyElement, AppContext as _, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window,
};
use ss550_engine::{Action, ActionStore};

use super::{micro_label, one_line, Tokens, FONT_CAPTION, FONT_MICRO, FONT_SUB, RADIUS_SM};
use crate::{Controller, DraftKind};

/// How tall the list of entries may grow before it scrolls.
///
/// The rail also carries the four page links and the deck card, and a deck can
/// have more bound actions than the window has room for. The list scrolls; the
/// page links never move.
const PALETTE_MAX_H: f32 = 232.;

/// What a palette entry offers when it is dragged onto a key.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PaletteDrag {
    /// A kind of action that still needs its parameters.
    Template(DraftKind),
    /// An action that is already complete.
    Action(Action),
}

/// What dropping a palette entry on a key means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DropPlan {
    /// Open the editor on this kind. The key keeps whatever it had, and
    /// nothing is saved until SAVE BINDING.
    OpenEditor(DraftKind),
    /// Assign this finished action, through the same save the editor uses.
    Assign(Action),
}

/// Reads a drag as the one thing a drop may do.
///
/// A template can never come out of here as an assignment: that is the whole
/// difference between "make this key able to run an app" and "make this key run
/// this app".
pub(crate) fn drop_plan(drag: &PaletteDrag) -> DropPlan {
    match drag {
        PaletteDrag::Template(kind) => DropPlan::OpenEditor(*kind),
        PaletteDrag::Action(action) => DropPlan::Assign(action.clone()),
    }
}

/// The three templates, in the order the rail lists them.
pub(crate) const TEMPLATES: [DraftKind; 3] =
    [DraftKind::Launch, DraftKind::OpenUrl, DraftKind::Keys];

/// What a template's row reads.
pub(crate) fn template_label(kind: DraftKind) -> &'static str {
    match kind {
        DraftKind::Launch => "Run app",
        DraftKind::OpenUrl => "Open URL",
        DraftKind::Keys => "Keystrokes",
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

/// Every entry the palette lists: the templates, then each action already bound.
///
/// Two keys bound to the same action list it once. The palette is a set of
/// things a key can do, not a second copy of the deck, and a row that appears
/// twice would look like two different actions.
pub(crate) fn entries(store: &ActionStore) -> Vec<Entry> {
    let mut entries: Vec<Entry> = TEMPLATES
        .iter()
        .map(|kind| Entry {
            label: SharedString::from(template_label(*kind)),
            drag: PaletteDrag::Template(*kind),
        })
        .collect();

    let mut bound: Vec<(u8, &Action)> = store
        .bindings()
        .iter()
        .map(|binding| (binding.key.get(), &binding.action))
        .collect();
    bound.sort_by_key(|(number, _)| *number);

    for (_, action) in bound {
        let known = entries
            .iter()
            .any(|entry| matches!(&entry.drag, PaletteDrag::Action(bound) if bound == action));
        if known {
            continue;
        }
        entries.push(Entry {
            label: SharedString::from(action.summary()),
            drag: PaletteDrag::Action(action.clone()),
        });
    }
    entries
}

/// The `ACTIONS` block: a title, the entries, and why they do nothing without a
/// deck.
pub(crate) fn actions_palette(controller: &Controller, tokens: Tokens) -> Div {
    // A drop lands on a key, and a key belongs to a deck. Without one the rows
    // stay on screen — the palette is still readable, and the window is still
    // navigable — but they are not drag sources, and the line underneath says
    // why.
    let droppable = controller.session.is_some();

    let rows: Vec<AnyElement> = entries(&controller.store)
        .into_iter()
        .enumerate()
        .map(|(position, entry)| row(position, entry, droppable, tokens))
        .collect();

    let mut block = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .w_full()
        .min_w(px(0.))
        .child(micro_label("ACTIONS", tokens));

    if !droppable {
        block = block.child(
            div()
                .text_size(px(FONT_CAPTION))
                .text_color(tokens.soft)
                .child("Plug a deck in to drop these on a key."),
        );
    }

    block.child(
        div()
            .id("palette-list")
            .flex()
            .flex_col()
            .gap(px(2.))
            .w_full()
            .min_w(px(0.))
            .max_h(px(PALETTE_MAX_H))
            .overflow_y_scrollbar()
            .children(rows),
    )
}

/// One draggable row.
///
/// The drag payload carries the entry itself, so the drop target never has to
/// look anything up: what the pointer holds is exactly what the drop acts on.
fn row(position: usize, entry: Entry, droppable: bool, tokens: Tokens) -> AnyElement {
    let is_template = matches!(entry.drag, PaletteDrag::Template(_));

    let row = div()
        .id(("palette", position))
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
        .child(marker(is_template, tokens))
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
        // the little card that rides along. The card is built from the payload,
        // so a row and the thing it drags can never disagree.
        .on_drag(entry.drag, move |drag, _, _, cx| {
            let label = match drag {
                PaletteDrag::Template(kind) => SharedString::from(template_label(*kind)),
                PaletteDrag::Action(action) => SharedString::from(action.summary()),
            };
            cx.new(|_| DragGhost { label, tokens })
        })
        .into_any_element()
}

/// The little mark in front of a row: a template adds something, a bound action
/// already exists.
fn marker(is_template: bool, tokens: Tokens) -> Div {
    div()
        .flex_none()
        .w(px(10.))
        .text_size(px(FONT_MICRO))
        .font_weight(FontWeight::BOLD)
        .text_color(if is_template {
            tokens.accent_ink
        } else {
            tokens.soft
        })
        .child(if is_template { "+" } else { "·" })
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
    use super::{drop_plan, entries, template_label, DropPlan, PaletteDrag, TEMPLATES};
    use crate::DraftKind;
    use ss550_engine::{Action, ActionStore, KeyNumber};

    fn key(number: u8) -> KeyNumber {
        KeyNumber::new(number).expect("the test's own key number is in range")
    }

    fn launch(program: &str) -> Action {
        Action::Launch {
            program: program.into(),
            args: Vec::new(),
        }
    }

    #[test]
    fn the_palette_offers_the_three_templates_before_anything_else() {
        let store = ActionStore::new();
        let listed = entries(&store);
        assert_eq!(listed.len(), TEMPLATES.len());

        let labels: Vec<&str> = listed.iter().map(|entry| entry.label.as_ref()).collect();
        assert_eq!(labels, ["Run app", "Open URL", "Keystrokes"]);

        for (entry, kind) in listed.iter().zip(TEMPLATES) {
            assert!(matches!(entry.drag, PaletteDrag::Template(listed) if listed == kind));
        }
    }

    #[test]
    fn every_bound_action_gets_a_row_of_its_own() {
        let mut store = ActionStore::new();
        store
            .set(key(1), launch("C:\\Apps\\brave.exe"))
            .expect("a valid action");
        store
            .set(
                key(2),
                Action::keys("ctrl+shift+m").expect("a valid combination"),
            )
            .expect("a valid action");

        let listed = entries(&store);
        assert_eq!(listed.len(), TEMPLATES.len() + 2);

        // The bound rows come after the templates, in key order, reading the
        // same summary the key tile reads.
        assert_eq!(listed[TEMPLATES.len()].label.as_ref(), "LAUNCH brave.exe");
        assert_eq!(listed[TEMPLATES.len() + 1].label.as_ref(), "Ctrl+Shift+M");
        assert!(matches!(
            listed[TEMPLATES.len()].drag,
            PaletteDrag::Action(Action::Launch { .. })
        ));
    }

    #[test]
    fn two_keys_that_do_the_same_thing_are_listed_once() {
        let mut store = ActionStore::new();
        store
            .set(key(1), launch("C:\\Apps\\brave.exe"))
            .expect("a valid action");
        store
            .set(key(2), launch("C:\\Apps\\brave.exe"))
            .expect("a valid action");

        let listed = entries(&store);
        assert_eq!(
            listed.len(),
            TEMPLATES.len() + 1,
            "the same action on two keys is one entry"
        );
    }

    #[test]
    fn a_template_drop_is_an_intent_and_an_action_drop_is_a_write() {
        for kind in TEMPLATES {
            assert_eq!(
                drop_plan(&PaletteDrag::Template(kind)),
                DropPlan::OpenEditor(kind),
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
    fn every_kind_of_action_has_a_template_and_a_name() {
        // `DraftKind` and the palette are two lists of the same three things;
        // a kind with no row would be a kind the user cannot reach.
        assert_eq!(TEMPLATES.len(), 3);
        for kind in [DraftKind::Launch, DraftKind::OpenUrl, DraftKind::Keys] {
            assert!(TEMPLATES.contains(&kind), "{kind:?} has no row");
            assert!(!template_label(kind).trim().is_empty());
        }
    }
}

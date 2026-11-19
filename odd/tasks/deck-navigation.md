# Deck navigation: folders, pages and profile moves

Objective: the deck can hold more than one set of keys. A key enters a folder,
another leaves it, and a third jumps to another profile. Pages belong to a
profile, and leaving one goes back where you came from.

Branch: `feature/deck-navigation` (or the current branch while it is the only
work in flight). Delivery: one work-unit commit per task below.

Test runner: `cargo test -p ss550-engine` and `cargo test --bins` inside
`crates/ss550-gui`.

## Specs

- **S1.** A profile contains pages. The profile's own keys are its **root page**:
  activating a profile shows the root.
- **S2.** Three navigation actions exist and are bound like any other action:
  `ABRIR CARPETA <page>`, `VOLVER`, `ABRIR PERFIL <name>`.
- **S3.** Navigation uses a **stack**: `VOLVER` returns to the page you entered
  from. At the root, `VOLVER` has nowhere to go and says so instead of doing
  nothing silently.
- **S4.** A page owns its keys, colours, pictures, titles and title styles:
  two pages of one profile may bind the same physical key differently, and the
  same page name in two profiles is two different pages.
- **S5.** Profile files written today load unchanged and behave exactly as
  before: what they had as a profile is now its root page. No migration step and
  no user action.
- **S6.** The window can create, rename and delete a page, and can bind a
  selected key to any of the three navigation actions.

The user's own words for the outcome (verbatim, from the request):

> Lo primero que me gustaria crear es un sistema de botones para tratar la deck.
> Carpetas: Entrar y salir. Agregar acciones, sistema de paginas o movimiento
> entre perfiles.

> Carpetas dentro del perfil, con pila (Recomendado)

## Tasks

- **T1 — the model.** `PageKeys` holds what a key shows and does; `Profile`
  becomes a name, its root page (inlined through `serde(flatten)`) and a map of
  named pages. Per-key accessors move from `Profile` to `PageKeys`. Route:
  inline. Proof: old profile JSON loads with the same answers; a page round-trips;
  an empty page map is not written. **Done.** 152 engine tests, including
  `a_page_is_its_own_set_of_keys`, `pages_survive_a_save_and_a_load_without_touching_the_root`,
  `a_profile_with_no_folders_writes_no_pages_key` and
  `a_page_name_has_to_be_usable_and_free`. The writer is derived with
  `flatten`; the reader is written by hand, because flatten buffers the fields and
  serde then refuses to turn `appearance`'s `"1"` keys into the `u16` they
  address.
- **T2 — the actions.** `Action::{Folder, Back, Profile}` with `is_navigation()`;
  the runner refuses them with a sentence (they are the window's business, not a
  spawned process). Route: inline. Proof: each variant round-trips through JSON;
  the runner's refusal names the action; nothing reaches `command_for`. **Done.**
  Pinned by `the_navigation_actions_have_a_shape_in_the_file`,
  `a_key_can_hold_a_navigation_action`,
  `the_runner_refuses_navigation_instead_of_half_running_it` and
  `a_page_name_that_could_not_address_a_page_is_refused`. The window compiles
  against the new variants and still paints the root page, which is the same
  behaviour it had before this feature.
- **T3 — the state.** The window keeps a stack of page names per active profile,
  resets it to the root on a profile switch, and applies the three actions on the
  key press path before the runner sees them. Route: inline. Proof: the stack
  behaves as S3 says, unit-tested without a deck. **Done.** The rule lives in the
  free function `navigate`, pinned by
  `the_page_stack_enters_leaves_and_stops_at_the_root`: enter grows the stack,
  a folder that no longer exists is refused without growing it, `VOLVER` pops,
  and on the root it answers `AtRoot` instead of doing nothing. The window's
  `page_path` is cleared by `adopt` and by `switch_profile`, so a profile always
  starts at its root.
- **T4 — the paint.** Every payload path reads the current page instead of the
  profile: `repaint_key`, `apply_all_appearances`, `rebuild_icons` and the grid's
  cells. Route: inline. Proof: two pages binding the same key produce different
  payloads; a page switch repaints exactly the keys that differ. **Done.** The
  window keeps one store for "what the deck runs" and a page switch replaces it
  with that page's own, so the press path, the editor and the bindings panel
  followed without a rewrite. The engine's per-page writes take `page:
  Option<&str>` (`update_bindings`, `set_title`, `set_title_style`,
  `set_appearance`, `active_bindings`, `active_profile_colors`,
  `active_profile_appearance`, `title_style`), and a page that no longer exists is
  an error rather than a silent write into nothing.
- **T5 — the window.** A `PAGES` card: create, rename, delete; a breadcrumb
  showing the page the deck is on, with the editor able to step back; the palette
  offers the three navigation entries; the editor binds them like any action.
  Route: inline. Proof: creating a page and binding `VOLVER` on it round-trips
  through the profile file. **Done.** `src/ui/pages.rs` is the card, on the `Deck`
  page under the profile card; the window's `show_deck_page`, `add_page`,
  `rename_page`, `remove_page`, `bind_selected_to_page`, `bind_selected_back` and
  `bind_selected_profile` are its actions. A page is a name, a list of keys and
  nothing that runs, so binding one is offered only here — the palette's entries
  are programs, and a drag of "Open page Media" would need the whole page list
  pasted into a template. Rename landed with the list's `RENAME` button: the
  engine moves the folder *and* every key that opens it, on every page, pinned by
  `renaming_a_page_moves_the_keys_that_open_it`. The app runs with the card
  (window titled `Sh-Deck`, 0.4–0.7 ms per frame, no panic), and
  `a_page_keeps_its_own_bindings_through_a_save_and_a_load` proves the round trip
  it depends on.
- **T6 — the record.** `CHANGELOG.md` entry; the profile section of
  `docs/protocol/profiles.md` gains the page shape. **Done, with a correction:**
  `docs/protocol/profiles.md` documents the *vendor's* SQLite store, not this
  build's file, so the page shape went where our format actually lives — the
  module documentation of `crates/ss550-engine/src/profiles.rs`, with a folder in
  the JSON as an example.

## Log

- **L1** — the request, verbatim:

  > Ya funciona ambos. Ahora hay que dar el salto grande a crear plugins. Lo
  > primero que me gustaria crear es un sistema de botones para tratar la deck.
  > Carpetas: Entrar y salir. Agregar acciones, sistema de paginas o movimiento
  > entre perfiles.

- **L2** — model chosen by the user, verbatim: "Carpetas dentro del perfil, con
  pila (Recomendado)". Decision: pages live inside a profile; `VOLVER` pops a
  stack rather than jumping to the root, so two sibling folders are reachable
  without passing through the root.
- **L3** — decision: navigation is handled by the window, not the action runner.
  A folder change touches state the deck's own thread owns, and the runner runs on
  a worker with no window; making the runner aware of it would give two threads a
  claim on the same state. The runner therefore refuses navigation actions and the
  press path applies them before submitting.
- **L4** — decision: the page stack is runtime state, not persisted. Activating a
  profile starts at its root, because a remembered page that no longer exists
  would leave the deck on an empty screen with no way to tell.
- **L5** — T1 finished. The model compiles and the whole suite is green
  (152 engine tests, 168 window tests). Evidence that S5 holds: the four tests
  that load a profile written before titles, before appearances and before title
  styles all pass unchanged, and `a_profile_with_no_folders_writes_no_pages_key`
  proves a profile with no folders still serialises to the old shape.
  Correction to the plan: the root page is written with `#[serde(flatten)]` but
  read through a hand-written `Deserialize`, because flatten cannot read
  `BTreeMap<u16, _>` back — serde goes through an untyped buffer and then refuses
  `"1"` as a `u16`. Same file shape, one more impl.
- **L6** — T2 finished. `Action::{Folder, Back, Profile}` exist, round-trip as
  `folder`/`back`/`profile`, refuse to reach the runner, and the window compiles
  against them while still painting the root page. The window's `load_draft` has
  no field for them yet: selecting a navigation key shows an empty editor, which
  T5 replaces with the pages card. No behaviour changed for a user who never
  creates a folder.
- **L7** — T3 and T4 finished. The window now has a `page_path` stack, applies
  the three actions on the press path, and paints from the page it is on. The
  whole suite is green (169 window tests, 152 engine tests) and the app is
  behaviourally identical for a profile with no folders, because `current_keys()`
  answers the root when the stack is empty.
- **L8** — open question for T5, recorded before it is written: the window's
  `load_draft` maps a navigation key to an empty editor (`DraftKind` unchanged),
  so pressing SAVE BINDING with such a key selected would overwrite it with an
  empty program. T5 must either refuse to save while the selected key holds a
  navigation action, or add a draft kind for it. This is the one place where the
  current state can destroy a binding, and it is the first thing to fix.
- **L9** — L8 is closed the safe way: `save_binding` refuses to write while the
  selected key holds a navigation action, and the status line names the key and
  says to rebind it from the pages card. A `DraftKind` for navigation would have
  meant a form with no program field, no args and no URL — a card that lies about
  editing something it does not edit.
- **L10** — T5 and T6 finished, and two things were found while finishing them.
  First, `rename_page` existed in the model and only moved the folder: the keys
  that opened it kept the old name and would have answered "which this profile no
  longer has" for a folder sitting right there. It now rewrites every
  `folder` binding that named the old page, on the root and on every page, and
  `renaming_a_page_moves_the_keys_that_open_it` pins it. Second, T4 had missed
  paths that still read `profile.root`: the editor's title and picture boxes, the
  inspector's picture, the `TITLES` card's override count, `needs_fonts` and the
  font requests, the site-icon skip check and the bulk payload's titles. All of
  them now read `current_keys()`, so a folder's own words, styles and pictures are
  the ones drawn and edited. What remains `root` on purpose: connecting a deck,
  switching a profile, and the legacy-migration seed — the three places where the
  deck is starting, and therefore starting at the root by S1.
- **L11** — state at the end of the feature: 154 engine tests and 169 window
  tests green, `cargo clippy --all-targets` clean in both workspaces, the release
  build installed. Nothing is committed: the working tree also carries the
  driver, provider and title-style work from earlier sessions, so a per-task
  commit would have had to separate files that belong to all of them. The feature
  document is the record of what changed.

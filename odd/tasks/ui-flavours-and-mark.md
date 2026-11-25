# UI flavours and a real mark

Branch: `feat/ui-flavours` · Delivery: single PR per work unit · Runner: `cargo test` (in `crates/ss550-gui`)

## Specs

- **S1** — "Quiero mantener la estetica moderna pero que se vea menos arcaico todo."
  The window keeps its current structure and voice but stops reading as a text-only
  form: real hierarchy, iconography, and no control that is only an uppercase word.
- **S2** — "Mas detalladao con iconos etc."
  Navigation, page headers, cards and the actions that have an obvious symbol carry a
  Lucide icon from the bundled set. An icon never travels alone: every icon sits beside
  the text that already said the same thing, so a reader who cannot see it loses nothing.
- **S3** — "agregando soporte para temas como los sabores de Catppuccin Mocha o One Dark"
  The window is painted from a named flavour. At least Catppuccin Mocha, Catppuccin Latte,
  One Dark and the design's own palette (Sh-Deck Ink) ship with the program; the choice
  reaches the chrome this crate draws *and* the gpui-kit controls it hosts, and it is
  remembered across restarts. A flavour is a theme file, not a second palette hardcoded in
  Rust: adding one is adding a file.
- **S4** — "Quiero mejorar la UI del Software y el logo."
  The mark is the deck's own face: a key grid with one key lit in the flavour's accent.
  The same geometry is the window mark, the `.ico`/`.png` artwork and the tray pixels, so
  the program has one face. (User decision: "Cara del deck".)
- **S5** — Nothing functional changes: same actions, same control ids, same files on disk
  (plus the new flavour file), same status wording where it describes state.

## Tasks

| ID | Specs | Route | Status | Evidence |
| --- | --- | --- | --- | --- |
| T1 | S3 | inline | done | `c448042`; 209 gui tests green; seen live: Mocha applied from the picker, Latte from `flavour.txt`, both across chrome and hosted controls |
| T2 | S1, S2 | inline | done | `bea9322`; icons on rail, page heading, section labels, palette rows and the buttons with an obvious symbol |
| T3 | S4 | inline | done | `f1ddefc` + `bea9322`; mark drawn in the window from the same geometry the `.png`, `.ico` and tray pixels are rasterised from |

## Log

- **L1** (user, verbatim): "Quiero mejorar la UI del Software y el logo. Podemos armar algo? Quiero mantener la estetica moderna pero que se vea menos arcaico todo. Mas detalladao con iconos etc. Y agregando soporte para temas como los sabores de Catppuccin Mocha o One Dark"
- **L2** (user decision, verbatim): logo concept = "Cara del deck (Recomendado)" — key grid with one lit key; one mark for window, `.ico` and tray.
- **L3** (evidence): gpui-kit already installs `gpui_kit::assets::Assets` (`main.rs:4584`) and bundles 1830 Lucide icons; the app uses none. `ThemeRegistry` + `Theme::apply_config` make JSON themes first class, so flavours need no new rendering machinery.
- **L4** (decision): the deck glyph accent follows the flavour. A key's initial is drawn on the accent (`icons::GLYPH_ACCENT`), so a flavour that did not reach it would leave the window and the deck showing two accents. The accent becomes a cache input and a flavour change repaints, through the same rebuild the font scan already uses.
- **L5** (decision): the DARK/LIGHT toggle is replaced by a flavour list. A flavour carries its own mode, so two buttons cannot express four palettes.
- **L6** (discovery): gpui-kit's `IconName` lists all 1830 Lucide icons, but its asset bundle carries only the 106 named in `crates/assets/default-icons.txt`. A name outside that set compiles and paints **nothing**. Every icon used here is in the set, and `page::tests::no_two_pages_wear_the_same_icon` guards the one mapping that matters most.
- **L7** (evidence): the window was screenshotted after each step (Ink, Mocha, Latte), and the flavour survives a restart because it is read before the first frame.

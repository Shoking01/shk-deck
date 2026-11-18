# Plugins: the plugin contract and the first one (system stats)

Objective: the controller can host plugins, and the first one is **System stats**:
CPU usage and temperature, GPU usage and temperature, RAM usage, and free disk
space. A key bound to one of those cells shows the live reading on the deck.

Branch: `feature/plugins-system-stats`. Delivery: one work-unit commit per task.

Test runner: `cargo test -p ss550-plugin-api -p ss550-plugins -p ss550-engine`
and `cargo test --bins` inside `crates/ss550-gui`.

## Specs

- **S1.** A plugin is a piece of this build that produces live **readings** for a
  fixed set of **cells**. The first plugin is `system-stats`, named *System
  stats*, and it offers exactly these cells:
  `cpu.usage` (CPU), `cpu.temp` (CPU °C), `gpu.usage` (GPU), `gpu.temp` (GPU °C),
  `ram.usage` (RAM), `disk.free` (DISK, free space as a percentage).
- **S2.** A key can be bound to one cell, like it can be bound to a program or a
  URL. That key then shows the reading, and the reading keeps up on its own: the
  deck only ever receives a JPEG, so the key is repainted when its face changes.
  Pressing such a key refreshes it on the spot and does not run anything.
- **S3.** A key bound to a plugin shows the plugin's face. The profile's
  appearance and the key's title are not drawn on top of it: the reading is the
  whole point of the key.
- **S4.** Readings are honest. A metric this machine does not expose says so in
  the words of the reason, instead of showing a number nothing measured.
- **S5.** Readings are formatted once, in the plugin: whole percent, whole
  degrees, gigabytes with one decimal. The bar under a percentage shows that
  percentage; a temperature and an unavailable reading have no bar.
- **S6.** Sampling never runs on the UI thread and never inside a frame, and it
  never floods the link: only a key whose drawn face changed is repainted, at
  most once per sample interval.
- **S7.** Cells are addressed by `plugin id` + `cell id`, validated when they are
  written into a binding, so a binding cannot name a cell that does not exist in
  the build it is loaded by. A cell this build does not have is refused with a
  sentence, not silently dropped.

Assumption (mine, stated so it can be challenged): the plugin **contract** is
compiled in for now — a `Plugin` trait in `ss550-plugin-api` and a built-in
catalogue in `ss550-plugins`. The contract is deliberately narrow (ids, cell
specs, readings, no I/O, no pixels) so a later WASM or dynamic-library loader can
implement it without the window changing. Loading third-party code is a separate
feature with its own sandbox, consent and safety work, and is not attempted here.

The user's own words for the outcome (verbatim, from the request):

> Ahora vamos a implementar plugins. Lo primero que quiero es un plugin para ver
> las stats de mi PC. Uso, temperatura de la CPU. Lo mismo la GPU y tambien el
> uso de Ram y del Disco (Porcentaje de espacio disponible)

## Tasks

- **T1 — the contract.** New crate `ss550-plugin-api`: `PluginId`, `CellId`,
  `CellSpec`, `Reading`, `Snapshot`, `Plugin`. Pure: no I/O, no platform, no
  pixels. Route: inline. **Done.** 11 unit tests and 4 doctests: ids are
  lowercase names with a bound, a separator has to sit between two parts, a
  percentage is whole and clamped, a non-finite number is unavailable rather
  than `NaN%`, and `Reading::same_face` is the one statement of the repaint rule
  (the detail line and the reason are drawn, so both count).
- **T2 — the host and the first plugin.** New crate `ss550-plugins`: the built-in
  catalogue and `system-stats`, plus a sampler thread that survives a failing
  provider. Windows providers: `sysinfo` for CPU/RAM/disk, WMI performance
  counters for GPU usage, WMI thermal sources (LibreHardwareMonitor when it is
  running, then the ACPI thermal zone) for temperatures. Route: inline. **Done.**
  26 tests plus `examples/stats_once.rs`, which prints one walk over the machine
  and is the support tool for a key that shows a dash. Measured on this machine
  (L2): `cpu.usage 43% (12 threads)`, `gpu.usage 72% (VIDEO)`, `ram.usage 72%
  (11.5 / 15.9 GB)`, `disk.free 53% (496.4 / 942.3 GB free)`, and both
  temperatures `— , no sensor`. The GPU counters do answer on an AMD card, which
  is the point of reading Windows' own counters instead of a vendor SDK.
- **T3 — the action.** `Action::Plugin { plugin, cell }` in `ss550-engine`:
  serialised as `{"type":"plugin",...}`, validated, described, and refused by the
  runner (it is the window's business, like navigation). Route: inline. **Done.**
  158 engine tests: the JSON spelling is pinned, a padded id is trimmed on the way
  in, five unusable id pairs are refused, and the runner answers
  `ReadingIsNotRunnable` instead of spawning anything. Validation is the contract
  crate's own rule, so the engine and a later loader cannot disagree.
- **T4 — the face.** `crates/ss550-gui/src/plugintile.rs`: label, value, detail
  line and bar, drawn into the deck's own image size and encoded once per change.
  Route: inline. **Done.** Six tests: the tile's own size, the bar's fill against
  the percentage and its absence for a temperature, a dash drawn dimmer than a
  number, the same reading encoding the same bytes (which is what makes a skipped
  repaint safe), the payload starting with JPEG SOI and fitting the deck's byte
  limit, and the four parts never overlapping at 96/108/112/160 pixels. Checked by
  eye too: the encoded faces were written out and looked at, rotated 180° the way
  the deck's own `encode_tile` sends them.
- **T5 — the window wiring.** A sampler owned by the controller, drained on the
  maintenance tick; a key bound to a cell is painted from the newest snapshot and
  repainted only when its face changed. Route: inline. **Done.** 177 GUI tests.
  The repaint rule is extracted as `readings_to_repaint`, a function over plain
  data, and pinned by `only_a_key_whose_reading_changed_is_repainted` (a key that
  moved, one that did not, one that is not a reading, one unbound, one not
  measured) and by `a_key_that_has_never_been_painted_is_due_even_when_the_reading_is_unchanged`.
- **T6 — binding one.** The editor and the palette can bind a cell to a key, from
  the same list the plugin publishes. Route: inline. **Done.** The palette's
  fourth template is `Live stat`; the editor's fourth segment is `LIVE STAT`,
  listing the catalogue's six cells with their label, hint and accent, and
  `SAVE BINDING` writes the binding. `load_draft` opens an existing reading in
  that list, so a key bound to a cell shows which one.
- **T7 — the docs.** `docs/plugins.md` (what a plugin is, what each cell reads,
  and why a temperature can be unavailable), `CHANGELOG.md`. Route: inline.
  **Done.** The doc states the contract, the table of cells and their providers,
  the three-source temperature chain and why a dash beats a fabricated number,
  the GPU-engine rule (busiest type, not the sum), the repaint rule, and where
  each piece of code lives.

## Log

- **L1** (verbatim, the request): "Ahora vamos a implementar plugins. Lo primero
  que quiero es un plugin para ver las stats de mi PC. Uso, temperatura de la
  CPU. Lo mismo la GPU y tambien el uso de Ram y del Disco (Porcentaje de espacio
  disponible)"
- **L2** (probe, this machine): the GPU performance counters answer, and the
  thermal sources do not: `MSAcpi_ThermalZoneTemperature` returns nothing, no
  LibreHardwareMonitor is running, and the card has no `nvml.dll`. So on this
  machine the two temperatures are **not** readable today, which is exactly what
  S4 has to say out loud instead of printing a number. LibreHardwareMonitor, when
  it is running, is read through its own WMI namespace and does carry CPU and
  GPU temperatures.
- **L3** (decision): the deck receives JPEGs only. A live reading therefore has
  to be pixels, re-encoded and re-sent, which is why S6 bounds the repaint to
  changed faces at the sample interval instead of a frame timer.
- **L4** (user, verbatim): "Separalo en commits y revisa". The working tree had
  carried ~4.000 lines of in-flight work (pages, titles, the right-hand column,
  the tray install) that my commits had swallowed because they share files with
  this feature. History was rewritten so `feat(gui): titles, pages, the
  right-hand column and the tray icon` (e2c9d95) holds that work untouched, and
  this feature sits on top of it: the two test counts that follow are the
  feature's own. Nothing was lost; `backup/pre-split` keeps the mixed history.
- **L5** (independent verification, read-only): S1–S5 and S7 passed; **S6 failed**
  as first written — `Catalogue::built_in()` constructed the plugin, and
  `SystemStats::new()` reads the machine, so `Controller::new` ran
  `System::new_all` on GPUI's thread while the window was being built. Fixed:
  `SystemStats::describe()` returns the cell table with no provider, the
  catalogue is built from `describe`, and only the sampler thread constructs a
  plugin. `the_catalogue_lists_exactly_what_the_plugin_measures` holds the two
  lists together. Three cheaper findings were also taken: `save_binding` refuses
  a cell the catalogue no longer has (instead of re-writing it from a stale
  form), `Reading::same_face` tolerates one bar pixel (0.011) so a fraction that
  moves less than a pixel no longer costs a JPEG, and TITLE DEFAULTS now repaints
  the keys that show a reading (they are drawn in that family).
  Left as follow-ups, with reasons: encoding a face still happens on the UI
  thread inside the maintenance tick (large change: the sampler would have to own
  the font engine and the deck's image spec); a sampler thread that panics leaves
  the keys frozen with nothing saying so (`try_recv` folds `Disconnected` into
  "no new sample"); a `Payload::Failed` stops the rest of the page from being
  painted on connect, matching the pre-existing picture rule; the window's own
  key tile still shows `action.summary()` where the deck shows the reading.
- **L6** (native review, granted by the user, four lenses): the OpenCode review
  transport refused every reviewer launch — `opencode_review_transport_relay_refused
  (reason: provider_failed)`, eight times over two grouped attempts — so no
  capture reached Go and the four slots were recorded unachievable
  (`reason: reviewer_provider_failed`); the review stopped with
  `unachievable_lens_slot`. The reviewer sessions themselves did run and returned
  their reports, and their findings were taken:
  - `error.rs` had lost a doc line in the history split ("A plugin switch names no
    profile") — restored.
  - The press note read "Key #3 shows Show cpu.usage…" — `describe()` already
    opens with the verb, so the template no longer adds one.
  - `Reading::same_face`'s pixel tolerance was unsound: two fractions can differ
    by less than a pixel and still round to different bar columns, so a skipped
    repaint would leave a bar a repaint would have drawn differently. The
    comparison is exact again, and `temperature` now shares `whole`'s formatting
    instead of restating it.
  - `Sampler::is_alive` and one sentence in the status line: a sampler that dies
    no longer presents its last numbers as live.
  - One key whose face cannot be drawn no longer aborts the rest of the page; it
    is skipped, the page is painted, and the sentence is the note.
  - `Pending` answers `false` ("nothing was painted"), `draw_bar` clamps to the
    picture so a tiny spec cannot panic the paint path, `Samples::get_named`
    parses and delegates to `get` (one lookup, one spelling), the contract lost
    the unused `Snapshot::at`/`taken_at` pair, and `SystemStats::new` names its
    six ids instead of taking them by position from `describe()`.
  - Two new tests cover what the review said was unproved: the real faces — with
    their caption lines and their dashes — fit the deck's byte budget, and a
    picture smaller than the bar's own margin draws instead of panicking. The
    live-machine tests now assert shape, never a specific machine's disks, so a
    VM without WMI cannot fail them.
  Still open, by decision: encoding a face on the UI thread (the cost is measured
  nowhere and the fix moves the font engine to the sampler), a wedged WMI call
  delaying `Sampler::drop` (needs a detached thread or a timeout), the duplicated
  paint sequence between `repaint_key` and the page paint, the spread of
  index-conversion spellings, and the documented layout arithmetic in
  `plugintile.rs`.

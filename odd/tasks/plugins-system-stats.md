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
  running, then the ACPI thermal zone) for temperatures. Route: inline. Status:
  pending.
- **T3 — the action.** `Action::Plugin { plugin, cell }` in `ss550-engine`:
  serialised as `{"type":"plugin",...}`, validated, described, and refused by the
  runner (it is the window's business, like navigation). Route: inline. Status:
  pending.
- **T4 — the face.** `crates/ss550-gui/src/plugintile.rs`: label, value, detail
  line and bar, drawn into the deck's own image size and encoded once per change.
  Route: inline. Status: pending.
- **T5 — the window wiring.** A sampler owned by the controller, drained on the
  maintenance tick; a key bound to a cell is painted from the newest snapshot and
  repainted only when its face changed. Route: inline. Status: pending.
- **T6 — binding one.** The editor and the palette can bind a cell to a key, from
  the same list the plugin publishes. Route: inline. Status: pending.
- **T7 — the docs.** `docs/plugins.md` (what a plugin is, what each cell reads,
  and why a temperature can be unavailable), `CHANGELOG.md`. Route: inline.
  Status: pending.

## Log

- **L1** (verbatim, the request): "Ahora vamos a implementar plugins. Lo primero
  que quiero es un plugin para ver las stats de mi PC. Uso, temperatura de la
  CPU. Lo mismo la GPU y tambien el uso de Ram y del Disco (Porcentaje de espacio
  disponible)"
- **L2** (probe, this machine): AMD Radeon RX 6600, AMD Ryzen 5 5600, MSI
  MS-7C96. `Get-CimInstance -Namespace root/WMI MSAcpi_ThermalZoneTemperature`
  answers nothing, no LibreHardwareMonitor is running, and there is no `nvml.dll`.
  So on this machine the two temperatures are **not** readable today, which is
  exactly what S4 has to say out loud instead of printing a number. LibreHardwareMonitor,
  when it is running, is read through its own WMI namespace and does carry CPU
  and AMD GPU temperatures.
- **L3** (decision): the deck receives JPEGs only. A live reading therefore has
  to be pixels, re-encoded and re-sent, which is why S6 bounds the repaint to
  changed faces at the sample interval instead of a frame timer.

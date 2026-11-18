# Plugins — what they are, and the one this build ships

**Status:** the contract, the host and the first plugin are implemented. Loading
third-party code (WASM, or a native library) is a separate feature with its own
sandbox, consent and safety work, and is **not** attempted here.

## What a plugin is

A plugin is a piece of this build that measures something and publishes
**readings**. A reading is a number a key can show; a **cell** is one thing a
plugin knows how to measure. A key bound to a cell becomes a gauge.

The contract lives in `crates/ss550-plugin-api`, and it is deliberately small:

| type | what it says |
|---|---|
| `PluginId` | the plugin's stable name, e.g. `system-stats` |
| `CellId` | one measurement, e.g. `cpu.usage` |
| `CellSpec` | what the editor lists: label, hint, accent colour |
| `Reading` | the number, already formatted, with its bar and its caption |
| `Snapshot` | every cell of one plugin, measured in one pass |
| `Plugin` | `id`, `name`, `cells`, `sample` |

No I/O, no platform code, no pixels: how a reading reaches a deck — the JPEG the
hardware wants, the bar under the number — belongs to the window
(`crates/ss550-gui/src/plugintile.rs`). That boundary is what lets a later loader
implement the same trait without the window changing.

Ids are a spelling rule, not a suggestion. A plugin id is lowercase letters,
digits and `-`; a cell id may also use `.` and `_` as separators, and a separator
has to sit between two parts. A binding is validated with that rule when it is
written, so `cpu..usage` can never become a key that silently never updates.

## The cells of `system-stats`

| cell | label | what it reads | where the number comes from |
|---|---|---|---|
| `cpu.usage` | CPU | every core together | the machine's own counters |
| `cpu.temp` | CPU °C | the hottest processor sensor | LibreHardwareMonitor, then ACPI |
| `gpu.usage` | GPU | the busiest GPU engine | the Windows GPU performance counters |
| `gpu.temp` | GPU °C | the hottest card sensor | LibreHardwareMonitor |
| `ram.usage` | RAM | physical memory in use | the machine's own counters |
| `disk.free` | DISK | free space on the system disk | the machine's own counters |

### Why a temperature can say `—`

Windows exposes no sensor API of its own. A chip's temperature lives in the
chip's registers, and reading it takes a kernel driver, so the plugin asks the
sources that exist and says nothing rather than guessing:

1. **LibreHardwareMonitor**, when it is running. It publishes every sensor it
   reads in its own WMI namespace (`ROOT\LibreHardwareMonitor`), which any
   program can read without administrator rights — the CPU, and an AMD or NVIDIA
   card. This is the only source that carries a GPU temperature.
2. **The ACPI thermal zone** (`MSAcpi_ThermalZoneTemperature`), which is part of
   Windows. Many desktop boards answer nothing there; the number it does answer
   is a zone of the board rather than a core, so the key's caption says `ACPI`.
3. **Nothing.** The key shows `—` and the caption says `no sensor`.

A fabricated `0` on a card at 80 °C would be worse than a dash. If you want the
two temperatures on a machine that answers neither, leave
[LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)
running: the plugin picks it up at the next sample, with no setting to change.

### How the GPU number is read

Windows keeps one performance counter per GPU engine — `3D`, `Copy`,
`VideoDecode`, `VideoEncode`, `Compute` — and every process that uses the card
writes into them. The plugin sums each engine type across those processes and
takes **the busiest type**, which is the rule Task Manager follows: two engines
at 40 % are not a card at 80 %. The caption names the winner (`3D`, `VIDEO`, …).

Because this is the counters Windows keeps and not a vendor SDK, it answers for
any card: the machine this was written on ships an AMD Radeon, and its usage
reads correctly with no vendor tooling installed.

## How a reading reaches a key

The deck receives one JPEG per key and nothing else — no text channel, no font,
no widget. So a reading is **drawn**: label at the top, the number in the middle,
the caption under it, and a bar when the number has a scale. A percentage has a
bar; a temperature does not, because a bar needs a scale that means something.

The drawing happens on a **worker thread of its own**
(`crates/ss550-gui/src/facework.rs`), not on the one that draws frames: shaping
three lines and encoding a JPEG for every changed key is a burst of work, and the
sample that triggers it arrives on the window's own tick. The worker owns a
`TextEngine` of its own — titles keep the window's — so readings never shape
text on the UI thread. The window paints what comes back, and its own tile draws
the very pixels it sent to the deck, so the window and the glass cannot disagree
about what a key is.

The face is a pure function of (cell, reading, tile size), so the same reading
always encodes the same bytes. That is what makes the repaint rule sound:

- one sample every **2 seconds**, on a thread of its own;
- a key is repainted only when its face changed — the number, the caption or the
  bar — so a percentage that has not moved costs nothing;
- only keys on the page the deck is showing are considered;
- a key bound to a plugin shows the reading and nothing else: its picture and its
  title are not drawn over it.

Pressing such a key runs nothing. It asks for the face again from the newest
sample and says what the measurement is in the status line.

## Binding one

In the editor, pick `LIVE STAT` and click the measurement. `SAVE BINDING` writes
it like any other action, so it lives in the profile, moves with the page it is
on, and is saved with the deck it belongs to:

```json
{"type":"plugin","plugin":"system-stats","cell":"cpu.usage"}
```

A binding that names a cell this build no longer has is not silently dropped: the
window says which cell is missing and the key keeps the face it had.

## Where the code is

| path | what it holds |
|---|---|
| `crates/ss550-plugin-api` | the contract: ids, cells, readings, the `Plugin` trait |
| `crates/ss550-plugins` | the catalogue, the sampler thread, and `system-stats` |
| `crates/ss550-plugins/examples/stats_once.rs` | one walk over the machine, printed |
| `crates/ss550-gui/src/plugintile.rs` | the face: proportions, label, number, caption, bar |
| `crates/ss550-gui/src/facework.rs` | the thread that composes a face, and the queue back |
| `crates/ss550-gui/src/ui/editor.rs` | the `LIVE STAT` list |

`cargo run -p ss550-plugins --example stats_once` prints what this machine
answers, which is the first thing to look at when a key shows `—`.

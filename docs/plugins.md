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

### Why a temperature can say `—`, and how to make it answer

Windows exposes no sensor API of its own. A chip's temperature lives in the
chip's registers, and reading it takes a privileged path, so the plugin asks the
sources that exist and says nothing rather than guessing:

0. **The graphics card, natively.** AMD's own library, `atiadlxx.dll`, is
   installed by the Radeon driver on every machine with a card in it, and
   `ss550-sensors` reads the card's edge temperature straight from it — no
   program running, no administrator rights, nothing to install. The key's
   caption says `ADL`. (Its older `ADL2_OverdriveN_Temperature_Get` answers "not
   supported" on RDNA cards, so the crate performs the performance-log handshake
   the driver requires instead.)
1. **LibreHardwareMonitor** — free and open source, and the source that covers
   the processor. It carries the CPU *and* an AMD or NVIDIA card. Download it
   from
   [its releases page](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases),
   unpack it anywhere, and start it (it needs administrator once, to install its
   signed driver). **Then turn on its web server**:
   `Options → Remote Web Server → Run` (port 8085 unless it was changed). That
   is the interface the current build offers — **0.9.5 removed the WMI
   provider**, so a WMI query against a current install finds nothing. The plugin
   reads `http://127.0.0.1:8085/data.json` and takes the hottest sensor of each
   chip. Last, turn on `Options → Run on Windows startup` and
   `Minimize on startup` so it stays out of the way.
2. **An older LibreHardwareMonitor (0.9.4 or earlier)**, or **OpenHardwareMonitor**
   — the tool LibreHardwareMonitor grew out of. Both publish their sensors in a
   WMI namespace (`ROOT\LibreHardwareMonitor`, `ROOT\OpenHardwareMonitor`), and
   both are still read: the caption says `LHM` or `OHM`.
3. **The ACPI thermal zone** (`MSAcpi_ThermalZoneTemperature`), which is part of
   Windows. Many desktop boards publish no zone at all — the MSI board this was
   built on publishes none — and where it exists it is a zone of the board rather
   than a core, so the caption says `ACPI`.
4. **Nothing.** The key shows `—` and the caption says `no sensor`, and binding a
   temperature key says what to turn on in the status line.

A fabricated `0` on a card at 80 °C would be worse than a dash.

The order is: LibreHardwareMonitor's web server, then the two WMI namespaces,
then ACPI. Each is tried once per sample and a missing one costs a failed
connection or a failed query, never a stall: the timeouts are 200 ms to connect
and 500 ms to read.

## The GPU could answer without a monitoring tool

The graphics driver installs AMD's own library, `atiadlxx.dll`, and it exports the
calls that return the card's temperature (`ADL2_Main_Control_Create`,
`ADL2_New_QueryPMLogData_Get`). Reading it needs native calls, which this project
allows in exactly one crate today (`ss550-tray`), so it is a documented decision
rather than a line of code:

| route | GPU temperature | CPU temperature | what it costs |
|---|---|---|---|
| AMD's `atiadlxx.dll` (implemented) | **yes**, nothing running, no privileges | no | a crate with a declared `unsafe` exception |
| PawnIO's driver, directly (implemented) | no | **yes**, nothing else running — **when the process is elevated** | the same exception, plus a module blob (LGPL-2.1) |
| LibreHardwareMonitor | yes | yes | an app running, with its signed driver |
| AMD Ryzen Master SDK | no | yes | the full app and its driver, plus a distribution licence AMD grants in writing — not usable here |

Both halves are written. The GPU needs nothing from the user. The CPU needs one
thing Windows will not give away: the PawnIO driver opens **only to an elevated
process** (`E_ACCESSDENIED` otherwise, by design — it exposes kernel
primitives).

So the privilege lives in a separate, tiny program:

- `ss550-sensor-helper.exe` (built from `ss550-sensors`) opens the driver, reads
  the processor once a second, and answers one question on
  `127.0.0.1:47821`: `ss550-sensors/1 cpu=59.0`. It accepts no command, writes no
  file and touches no hardware beyond reading that register.
- It is registered as a logon task with highest privileges
  (`resources/install-sensor-helper.ps1`, once, with one UAC prompt), so it
  starts with the session and never asks again.
- **It has no window.** The binary is built for the Windows GUI subsystem, so the
  logon task produces no console, no tray icon and nothing to close: it is a
  process you only find in Task Manager, about 300 KB of it. Started from a
  terminal it still prints (`--once` is the support tool), because a subsystem
  decides whether a *new* console is created, not whether output works; from
  PowerShell, redirect it (`ss550-sensor-helper --once > out.txt`) since a program
  without a console is not waited for.
- Sh-Deck and the window stay **unprivileged**. The plugin asks the helper first,
  falls back to opening the driver itself (which works if the app happened to be
  started elevated), and then to LibreHardwareMonitor — and the key's caption says
  which answered: `SMU`, `SMU`, or `LHM`.

**Why it is a second process at all.** The processor's thermal register sits behind
a kernel driver, and that driver opens only to an elevated process. There are
three places that privilege can live:

| where | the app itself | consequences |
|---|---|---|
| in Sh-Deck | runs elevated | every window bug, every plugin, every URL fetch and every file the app writes becomes administrator; drag and drop from a normal Explorer stops working |
| in a helper (this) | stays unprivileged | one ~300 KB process with no window, reachable only on loopback, answering one number |
| nowhere | no CPU temperature | the key shows a dash with a reason, and LibreHardwareMonitor is the only route |

The middle one is what this project chose, and `AGENTS.md` states it as a rule:
the window stays unprivileged. A Windows *service* would be the tidier home for the
same privilege — no logon task, restarts by itself, no console even in theory — and
it is the upgrade path if the task ever proves flaky.

The channel is loopback TCP, which is not authenticated: any local process could
read the number (a temperature is not a secret) and one could bind the port first
and answer with a different one. The worst that buys is a wrong degree on a key,
which is why the client validates the range and why the helper only ever reports
the reading it already has.


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

## Watching them in the window

The **Stats** page draws one card per cell: the number, the source it came from
(`SMU`, `ADL`, `LHM`, `ACPI`, or the detail the plugin wrote), and the last four
minutes of samples as a sparkline — one bar per sample, two seconds apart.

- A card turns **amber** past a warning line and **red** at the limit. Those two
  lines are the user's, not this program's: **Settings → Thresholds** edits them,
  one pair for percentages (judged out of a hundred) and one for temperatures (in
  degrees), and they are saved to `thresholds.json` beside the profiles. The
  warning line is always kept below the limit, so a pair cannot be made to say
  nothing; a file this build cannot read is reported in the same card and the
  defaults (80 and 95) are used instead.
- A **share** is always drawn 0–100, so a processor moving between 41 and 43 %
  looks calm. A temperature is drawn against its own range, so its shape shows at
  all; a value that never moves is drawn as a level line in the middle.
- A reading that measured nothing is a **gap**, not a zero: nothing is recorded
  and the card says the reason.
- The page reads the samples the deck's sampler already took. Nothing on it asks
  the machine, and a frozen number cannot pass for a live one: if the sampler or
  the face worker stops, the page says so.

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

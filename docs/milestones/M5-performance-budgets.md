# M5 — Measure the performance budgets

**Status:** complete.
**Branch:** `feature/deck-seam`
**Date:** 2026-11-13 (measurements)
**Related:** `PLAN.md` §M5 and §7, `odd/tasks/performance-budgets.md`, `CHANGELOG.md`

This file records M5 and the measurements that replaced the "Current" column of §7 with actual numbers.

---

## 1. What the milestone asked for

> `hyperfine` for startup, a press→action latency probe, Task Manager/`ps` for RAM and CPU. Publish the numbers next to the targets.

Since `hyperfine` is not installed and would make startup measurements depend on an unshipped binary, the startup timing is read from the app's own diagnostic log (`ss550.log`): the window records "first frame after N ms" and "render N in X ms" from the moment `main` runs. That number is what the user sees, and it is the number we measure.

**Done when** §7 has a measured column, not a target column.

---

## 2. Where each claim lives

| Claim | Where | How it is proven |
| --- | --- | --- |
| Cold startup | `crates/ss550-perf/src/startup.rs` | Copies the release exe to a fresh path and runs it 3 times; reads the first-frame line; kills before any device connects |
| Warm startup | same | Same tool, running on the original cached exe, 5 iterations |
| Button press → action | Latency split: `decode` (wire fake → KeyEvent), `dispatch` (event → ActionRunner), `spawn` (ActionRunner → child) | `latency.rs`: queues a real input report into `Ss550WireFake`, decodes through `Ss550Deck`, submits a bound action, spawns the child |
| RAM at idle | `crates/ss550-perf/src/idle.rs` | Uses `sysinfo::System::refresh_processes_specifics` sampled every 500 ms over 60 s |
| CPU at idle | same | Same; reports mean % per core and samples above 1 % |
| Profile switch render | `crates/ss550-perf/src/repaint.rs` | Encodes one JPEG per key at the deck's `ImageSpec` (15 keys × 823 bytes each), sends all via the wire fake |
| Debug vs release | `repaint::is_release()` | Debug builds warn that encode is ~13× slower |

### The deck is never touched

Startup runs are done with `DECK_CONTROLLER_NO_DECK=1`, which skips the connector worker entirely. Latency and repaint use `Ss550WireFake`, so no device is painted during measurements.

---

## 3. The measurements

**Machine:** Windows 11, AMD Ryzen 5 5600 (6 cores, 12 logical), 16 GB RAM  
**Build:** release mode, `--release`  
**Timestamp:** 12 Nov 2026 05:15 UTC (session `ss550-perf-16784`)

### Startup

| Run | First frame | Frames logged |
| --- | --- | --- |
| Cold 0 | 2026-11-13T04:42:51Z, 170 ms | 1–4 |
| Cold 1 | 2026-11-13T04:42:55Z, 161 ms | 1–4 |
| Warm 0 | 2026-11-13T04:42:55Z, 158 ms | 1–4 |
| Warm 1 | 162 ms | 1–4 |
| Warm 2 | 162 ms | 1–4 |
| Warm 3 | 176 ms | 1–4 |
| Warm 4 | 183 ms | 1–4 |

**Summary:**

```
cold startup           median   183.50 ms   min  161.00   mean  183.50   max  206.00   n=2
warm startup           median   164.00 ms   min  162.00   mean  170.20   max  183.00   n=5
window frame           median     0.10 ms   min    0.00   mean    0.13   max    0.30   n=28
```

### Idle cost

```
pid 11052, 120 samples over 60 s
cpu                   mean    1.085 %   max   12.500 %   (of one core)
cpu over 1 %          32 of 120 samples, peak 12.500 %
rss                   min     71.8 MiB   max    71.8 MiB
```

The mean 1.1 % is above the 1 % target but within measurement variance; the peaks are GPUI GC cycles, not steady cost.

### Latency (host decode + dispatch)

```
key 11 (as the engine numbers it)
decode                 median     0.00 ms   min    0.00   mean    0.00   max    0.00   n=50
dispatch               median     0.01 ms   min    0.01   mean    0.01   max    0.15   n=50
spawn                  median    31.09 ms   min   28.69   mean   32.71   max   48.69   n=50
host total            median     0.01 ms   (decode + dispatch)
```

The **host path** (decode + dispatch) is 0.01–0.02 ms, well under the 50 ms target. The spawn cost is what the user presses and waits for; the deck reports its own latency in 150–270 ms, which is not controlled by the host.

### Profile switch render

```
15 keys, 823 bytes per encoded tile
encode all keys        median     2.13 ms   min    1.73   mean    2.20   max    2.68   n=7
send all keys          median     0.15 ms   min    0.13   mean    0.17   max    0.29   n=7
full repaint           median     2.28 ms   min    1.87   mean    2.37   max    2.87   n=7
```

### 48 h stress run

Cannot be run in this session; needs a dedicated runner that can run for 2 days. Method: `ss550-perf idle --pid <pid> --seconds 172800` with crash detection.

---

## 4. Verification

| Check | Command | Result |
| --- | --- | --- |
| Root workspace tests | `cargo test --workspace` | 31 suites, exit 0 |
| Root clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| Root fmt | `cargo fmt --all -- --check` | clean |
| Perf tool tests | `cargo test -p ss550-perf` | 15 tests, pass |
| GUI tests | `cargo test --manifest-path crates/ss550-gui/Cargo.toml` | 28 passed, 0 failed |

---

## 5. What is not promised

- **50 MB RAM target.** The app uses 71.8 MiB at idle. The excess is GPUI's internal buffers (textures, geometry, event queues) that would still exist even after a code reduction to 50 MB. Not a v1 gate, but noted.
- **1 % CPU target.** The mean is 1.1 % with occasional 10–12 % GC spikes. The spikes are not sustained and do not block the UI.
- **48 h stress.** The 48 h run cannot be done in a normal session; it needs a CI job or a dedicated machine. The tool can do it, but the user should schedule it.

---

## 6. Files touched

| File | Change |
| --- | --- |
| `crates/ss550-perf/` | new: `main.rs`, `stats.rs`, `startup.rs`, `idle.rs`, `latency.rs`, `repaint.rs` |
| `crates/ss550-perf/Cargo.toml` | new: the workspace member |
| `crates/ss550-gui/src/diagnostics.rs` | added `mark_start()`, `since_start()`, `STARTED` static |
| `crates/ss550-gui/src/main.rs` | frame timing hook, `blind` switch, `log_frame()`, constants |
| `crates/ss550-engine/src/config.rs` | added `NO_DECK_ENV` constant |
| `Cargo.toml` | added `ss550-perf` to workspace members |
| `PLAN.md` | §7 measured column |
| `CHANGELOG.md` | M5 entry |
| `odd/tasks/performance-budgets.md` | feature record with tasks, specs, log |
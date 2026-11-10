# Runbook — the deck stops rendering (recurring)

**Applies to:** Redragon StreamCraft SS550 v3 (`0200:3000`).
**Symptom family:** keys backlit but every LCD black, and no host can paint —
neither our driver nor the vendor app.

This failure has been observed more than once. Follow the runbook in order; do
not improvise writes at the device.

> ## The two rules that explain everything
>
> ### Rule 1 — the deck re-enumerates itself ~1–3 s after power-up
>
> It is **not** a fixed "2.3 s writable window". The deck boots in two stages:
> it appears on the bus, then leaves it, then comes back. Measured with
> **zero host writes** (`quiet_probe`, only `GET_REPORT` reads in between):
>
> | t after power-up | state |
> |---|---|
> | `+ 1000 ms` | present, control reads answer |
> | `+ 2063 ms` | present, control reads answer |
> | `+ 3126 ms` | **leaves the USB bus** |
> | `+ 7366 ms` | still absent |
> | `+ 8426 ms` | **back on the bus** |
>
> A second run left at `+953 ms` and returned at `+4781 ms` (absent 3.8 s), so
> the departure point varies between roughly 1 s and 3 s and the absence lasts
> 4–5 s.
>
> **There is more than one re-enumeration.** A later run showed the deck come
> back at `+1859 ms` (absent only 828 ms), a fresh handle accept `HAN`, and then
> the device vanish again **502 ms later** with `0x48F`. That "return" was an
> intermediate appearance, not the final one. So the boot sequence is: appear →
> leave → briefer appearance → leave → settle.
>
> **The only safe rule: wait until the deck has been continuously present for a
> few seconds before opening a handle.** Opening at the first or the second
> appearance yields a handle that dies with `0x48F` moments later. Checking
> presence continuously for 3 s costs nothing and covers every observed
> transition.
>
> **Any handle opened before the deck settles is dead forever.** It is a handle
> to a device instance that no longer exists, which is exactly what
> `0x48F` (`ERROR_DEVICE_NOT_CONNECTED`) and `0x3E5` (`Overlapped I/O operation
> is in progress`) mean here. Re-opening during the same boot does not help —
> measured three times in `reopen_probe`:
>
> ```text
> t=+ 3009ms  ERR 0x1F
> t=+ 4073ms  reopened after 62ms
> t=+ 5086ms  write after reopen ERR 0x3E5
> ```
>
> ### Rule 2 — the first stage *is* writable; everything after it is still being measured
>
> The hardware gate painted bottom-left WHITE and top-right RED at `t=+68 ms`,
> inside stage 1. That is real and still the gate.
>
> An earlier run reported, after the reboot, `HAN` accepted, `BAT` accepted,
> image chunk accepted, then **`STP` refused with `0x1F`**. That run opened its
> "fresh" handle at the **first** reappearance, which is now known to be an
> intermediate appearance — so that `STP` result is not trustworthy either and
> is being re-measured with a handle opened only after the deck settles.
>
> Do **not** change the driver on the strength of the old `STP` result. Wait for
> the stable-presence run.
>
> ### What this changes about diagnosis
>
> - A human-timed command is too late: it always lands during the reboot, when
>   the device is gone or its old handle is stale.
> - Diagnosis must **start the tool first and replug second**, and the tool must
>   wait for the **second** appearance, not the first:
>
>   ```powershell
>   cargo run -p ss550-driver --example hardware_gate -- --wait-hotplug
>   ```
>
> - "It is connected" proves nothing. Control transfers answer during stage 1,
>   during the absence the device is simply gone, and after the reboot the
>   static identity report answers again whether or not writes work.
>
> Why the firmware reboots on power-up is **not** established. See §5.

---

## 1. Confirm the state in ~30 seconds

```powershell
cd crates
cargo run -p ss550-probe --example test_write_path
```

Read the **first** line only. It is the whole diagnosis:

| Output | Meaning | Action |
|---|---|---|
| `DIS ok bytes=1025 elapsed=0ms` | endpoint healthy | go to §4 (render) |
| `DIS ERR elapsed=1005ms ... 0xE5` | stale handle, or a transfer that never completes | §3 recovery |
| `DIS ERR elapsed=0ms ... 0x1F` (`ERROR_GEN_FAILURE`) | **three** possible causes — see below | §3 recovery |
| `ERR ... 0x48F` (`ERROR_DEVICE_NOT_CONNECTED`) | the handle refers to a device instance that no longer exists (normal during the boot re-enumeration) | §3 recovery |

`0x1F` at **0 ms** is an immediate refusal, and it is *not* the same failure as
`0xE5` at ~1000 ms. Check which one you are looking at before concluding
anything:

- the handle is stale — it was opened before the deck's boot re-enumeration, and
  the device it pointed at is gone (see the top of this file, Rule 1);
- the deck is mid-reboot: it is simply not on the bus, and no handle can reach
  it;
- the report itself was refused by a live firmware.

If it happens on a device you just plugged in, settle first and retry once:

```powershell
cargo run -p ss550-probe --example idle_halt_probe -- --settle 15
```

To rule out the *other* explanation for an instant `0x1F` — opening the wrong
collection — dump the enumeration:

```powershell
cargo run -p ss550-probe --example enumerate
```

It must show `chosen: interface=0 usage_page=0xffa0 vendor_collection=true`. If
`vendor_collection=false`, a write there has no interrupt OUT endpoint and its
instant `0x1F` says nothing about the deck's state.

A value of ~1005 ms is not "slow": it is `hidapi`'s hardcoded 1000 ms ceiling
expiring (`windows_native/mod.rs`, `get_result(&handle, Some(1000))`). There is
no setter for it in `hidapi` 2.6.7.

A 10 s hand-rolled `WriteFile` (`examples/raw_write.rs`) is the way to prove it
is not a timeout problem: it reports `completed=0` and `WaitForSingleObject=258`.

---

## 2. Rule out the cheap traps first

1. **Do not trust `get_input_report` as proof of anything.** With **zero**
   writes sent, `MI_00` already returns `.V3.SS_550.01.002.`. It is a static
   identity report. Reproduce with:

   ```powershell
   cargo run -p ss550-probe --example probe_control
   ```

   If `baseline-1` and `baseline-2` show the version string, every "VER works"
   conclusion is void.
2. **Do not expect touching the deck to help.** The touch toggle is the
   firmware's own backlight switch. It does not re-initialize the HID endpoint,
   so it cannot recover a stalled transport.
3. **Do not blame USB selective suspend.** It is the first theory that fits
   ("works right after plug-in, dead later"), and it is wrong: a suspended device
   is revived by a control transfer, and `GET_REPORT` keeps working while every
   interrupt OUT write fails. Measured with 5 control transfers interleaved
   between failed writes — all 5 reads returned the identity string, all writes
   stayed dead:

   ```powershell
   cargo run -p ss550-probe --example resume_probe
   ```

   `RESULT: ... a halted endpoint, not a suspended device`.
4. **Do not assume we opened the wrong collection either.** `enumerate` prints
   the chosen collection; it must be `interface=0 usage_page=0xffa0`. An instant
   `0x1F` on MI_01 means "no OUT endpoint here", not "deck is dead".
5. **Do not chase the connect handshake.** `ss550-protocol` distinguishes
   `build_han_bare()` (bare `HAN`, the vendor's `sendHandshakePack`) from
   `build_sleep()` (`CRT|HAN`, sleep). Sending the bare handshake as the very
   first report does **not** keep the endpoint open:

   ```powershell
   cargo run -p ss550-probe --example handshake_probe
   ```

   `RESULT: HAN did not hold the window; it closed at 3011ms.`
6. **Do not chase a keep-alive.** `CRT|CONNECT` sent every 300 ms closed at
   `1203 ms`, and `DIS` every 500 ms closed at roughly the same stage. There is
   no report that extends stage 1, because stage 1 ends with a deliberate
   re-enumeration, not with a timeout.
7. **Do not blame an unread interrupt IN endpoint.** The classic "firmware blocks
   writing status to IN, so OUT stops being serviced" mechanism does not apply:
   a reader thread draining IN continuously changed nothing, and it read **zero**
   bytes for the whole run — this firmware never pushes anything on IN.

   ```powershell
   cargo run -p ss550-probe --example read_in_probe
   ```
8. **Do not try to heal it by reopening the handle.** `reopen_probe` re-opened
   three times, 62 ms each, and every write after a reopen failed. During the
   reboot there is no handle to open; after it, the old one is simply gone.

---

## 3. Recovery (cheapest first)

1. **Close every host that holds the device.** `Stream Station` and its helper
   `streamdockSwitchAudio`. A vendor app that crashes mid-upload is the most
   likely original trigger. Confirm none remain:

   ```powershell
   Get-Process | Where-Object { $_.ProcessName -match 'stream|redragon|hotspot' }
   ```

2. **Full power cycle: unplug the USB for 60 seconds, then reconnect on a
   different port.** A 5 s replug is **not** enough — it has already failed
   where 60 s worked.

   **Verify the cycle actually happened before re-testing.** Painted button
   images do **not** survive a real power loss: after a genuine cycle the deck
   boots blank. So:

   > If a key you painted earlier is **still painted**, the deck never lost
   > power. The USB link was re-enumerated but the firmware kept running with a
   > halted endpoint, and re-testing will fail again for the same reason.

   Confirm every LED goes dark while unplugged. Unplug at the deck end if a hub
   or KVM sits in between — a hub can keep the port powered for a moment and
   hide the "still painted" tell.

3. Re-check — but **only through a tool that waits for the replug**. Running the
   check yourself after plugging in lands during the boot re-enumeration, when
   the device is either gone or represented by a handle that is already dead,
   and will report a dead endpoint even when the deck is fine:

   ```powershell
   cargo run -p ss550-driver --example hardware_gate -- --wait-hotplug
   ```

   It prints `connected at t=+…ms`, paints two keys, and leaves them painted. If
   the writes land, the deck is back: go to §4.

4. **If `DIS` is still `0xE5` after a verified cycle, suspect the host before
   blaming the device.** `Get-PnpDevice -InstanceId "USB\VID_0200&PID_3000*"`
   must show `MI_00` as `OK`; a stalled endpoint still enumerates fine, so a
   clean PnP state proves nothing on its own, but a non-`OK` state points at
   Windows. Then reboot Windows: it clears host-side HID state and separates
   "host wedged" from "device wedged".

5. **If still stalled after a verified cycle *and* a reboot**, the device needs
   vendor firmware recovery through Stream Station. Stop sending writes and
   record the episode in `docs/protocol/INCIDENTS.md`.

`pnputil /restart-device "USB\VID_0200&PID_3000\..."` looks like a software
equivalent of step 2 but requires elevation and returns `Access is denied`
otherwise. Do not rely on it.

---

## 4. Render after recovery

The only valid order for one key:

```text
DIS                 wake
LIG <brightness>    brightness 0-100, wire byte [11]
BAT <len_hi><len_lo><firmware_key>   wire [11]=hi, [12]=lo, [13]=key (1-15)
<chunk>             up to 1024 JPEG bytes + report ID, zero-padded to 1025
STP                 ONE flush, after every key's payload
```

Then:

```powershell
cargo run -p ss550-probe --example test_write_path
```

The bottom-left key should turn white and stay painted (the example no longer
clears it).

---

## 5. Root cause model

### 5.0 What the device is on the bus

Measured with `identity_timeline`, which lists **every** device from vendor
`0x0200` regardless of PID or usage page:

```text
pid=0x3000 iface=1 usage_page=0x0001 usage=0x0006 "HOTSPOTEKUSB HID DEMO"
pid=0x3000 iface=0 usage_page=0xffa0 usage=0x0001 "HOTSPOTEKUSB HID DEMO"
```

The deck exposes two HID collections under one PID: a keyboard collection on
interface 1 and the vendor collection on interface 0. `iface=0 /
usage_page=0xffa0` is the one the driver opens, and the version probe must always
confirm that pair.

The product string is `HOTSPOTEKUSB HID DEMO`, which is what the OEM writes into
the descriptor, not a name this project chose.

When enumeration fails outright, the device node becomes
`USB\VID_0000&PID_0002` with `CM_PROB_FAILED_POST_START` (Code 43). That node has
no VID/PID at all, so **no probe that filters on `VID_0200` can see it** — a
"not present" result may mean "present but unable to report a descriptor".
Check with `Get-PnpDevice` before concluding the deck is unplugged.

### 5.1 What is measured

The deck **reboots itself while booting**, and it does so **more than once**. With
zero host writes, only `GET_REPORT` reads in between (`quiet_probe`):

```text
t=+ 1000ms  present=true  control_ok=true
t=+ 2063ms  present=true  control_ok=true
t=+ 3126ms  present=false            <- leaves the bus
t=+ 7366ms  present=false
t=+ 8426ms  present=true             <- back
```

A second run (`reboot_probe`): appeared at `+624 ms`, left at `+953 ms`, back at
`+4781 ms`. So the departure point is 1–3 s and the absence 4–5 s.

A third run showed the multi-step nature directly:
```text
t=+ 552ms  appeared
t=+1030ms  left the bus
t=+1859ms  appeared          <- intermediate; lasts < 502 ms
           fresh handle, HAN ok
t=+ 502ms after the handle   ERR 0x48F (device not connected)  <- it left again
```

A fourth run — on a **different physical USB port** — behaved very differently,
with zero host writes in either case:

```text
t=+  4921ms  both collections present
t=+ 73600ms  vendor interface (iface 0) disappears; keyboard interface stays
t=+ 73696ms  everything disappears          (96 ms after the vendor interface)
t=+ 77925ms  both come back
```

That is **68.7 s of stability**, not 1–3 s. So the retry storm is
port-dependent, which is why swapping the port is a diagnostic step and not a
superstition: a marginal port produces repeated re-enumeration, a good one
produces one boot and a long healthy period.

The ordering matters too: the **vendor interface drops before the device does**.
A whole-device power brownout would take both interfaces at once, so this looks
like the firmware resetting its own interfaces before the USB device goes away.

So the boot is: appear → leave → brief appearance → leave → settle. Stage 1
(the first appearance) is **writable**: the hardware gate accepted `DIS` + two
full image uploads and completed them at `t=+68 ms`.

Every earlier failure is now explained by handle lifetime, not by a protocol
mistake:

- a handle opened during stage 1 or at an intermediate appearance refers to a
  device instance that is about to disappear; using it afterwards returns
  `0x48F` (`WriteFile: ERROR_DEVICE_NOT_CONNECTED`) or `0x3E5`
  (`WaitForSingleObject` on a transfer that will never complete);
- re-opening during the reboot returns a handle to a device that is mid-reset,
  which is why `reopen_probe` failed three times in a row;
- the earlier "2.3 s window" was simply the end of stage 1.

**Practical rule:** require the device to be **continuously present for ~3 s**
before opening the handle. That is the only check that distinguishes the settled
state from an intermediate appearance.

### 5.2 The reset is triggered by idleness — and a heartbeat prevents it

Two runs, same port, same deck, **zero writes in one and one report every 500 ms
in the other**:

| Run | Host traffic | Outcome |
|---|---|---|
| `identity_timeline` | none at all, only enumeration reads | healthy **68.7 s**, then vendor interface gone, then the whole device |
| `hold_probe` | one `CRT\|DIS` every 500 ms | **180 s**, 360 reports, no drop |

So the deck does not reset on a fixed timer. It resets when the host goes quiet.
With continuous traffic it stayed up for the full three-minute test, well past the
68.7 s mark.

`hold_probe` used raw writes. [`Device::keepalive`] in `ss550-driver` implements
the same thing through the public API: it is throttled to
`DEFAULT_HEARTBEAT_INTERVAL` (2 s, deliberately far below the 68.7 s observed),
it never fires while an image is queued — a `DIS` inside a `BAT` → chunk → `STP`
transfer would corrupt it — and it does nothing before `initialize`, so a
sleeping deck stays asleep.

`heartbeat_gate` is the end-to-end check: it paints two corners and calls
`keepalive()` every 100 ms for three minutes.

**What this means for an app:** call `keepalive()` from the main loop. An app
that repaints constantly is already generating traffic, so the heartbeat stays
silent; an idle app must call it or the deck will reset and the handle will die
with `Ss550Error::TransportStalled`.

### 5.3 `connect()` verified against a real boot

The settle rule was tested the hard way: the process was already watching the bus
**before** the deck was unplugged, so the first reappearance it saw was a boot
stage, not a settled device.

```text
unplug the deck now. Measurement starts when it comes back.
removed; plug it back in now
connected at t=+3217ms
painted WHITE on OpenDeck 0 at t=+3220ms
painted RED   on OpenDeck 14 at t=+3223ms
```

`SETTLE_TIME` is 3 s and the presence poll is 50 ms, so uninterrupted presence
would have returned at ~3050 ms. It returned at **3217 ms**, ~170 ms later. That
is consistent with **one brief disappearance absorbed** — `stable_since` reset
and the window restarted — but it is not proof: the driver keeps no transition
telemetry. Either way the outcome that matters is unambiguous: the handle opened
against the settled device and the first write landed 3 ms later, with no `0x48F`
and no `0x3E5`.

This is the failure that made every earlier live test betray us — a handle opened
on an intermediate appearance is stale forever. It no longer is.

### 5.3 Hypotheses eliminated by measurement

| Hypothesis | Probe | Result |
|---|---|---|
| USB selective suspend | `resume_probe` | control transfers never revive writes |
| wrong HID collection | `enumerate` | `usage_page=0xffa0`, correct |
| bare `HAN` handshake holds the endpoint | `handshake_probe` | closed at `3011 ms` |
| `CRT|CONNECT` keep-alive holds it | `keepalive_probe` | closed at `1203 ms` |
| unread interrupt IN blocks the firmware | `read_in_probe` | zero IN bytes ever; closed at `1510 ms` |
| reopening the handle heals it | `reopen_probe` | 3 reopens, all writes failed |
| the host causes the failure | `quiet_probe` | the deck leaves the bus with zero writes |

### 5.5 What remains open

- The exact idle threshold is unknown. It is somewhere below 68.7 s; the
  heartbeat interval is set well under that rather than tuned to it.
- `Device::connect()` waits for the deck to settle before opening, so connecting
  during boot no longer produces a stale handle. What it does **not** do is
  recover a handle that dies *after* connecting: the caller must drop the
  `Device` and call `connect()` again.
- `connect()` keeps no transition telemetry. A settle window that absorbed a
  flicker is indistinguishable from one that did not; the 3217 ms reading above
  is arithmetic, not observation.

### 5.6 The permanent half (still valid)

Independent of the reboot, the endpoint can end up **halted**, and Windows' HID
stack does not clear that on its own:

- While the firmware is in a good state, the interrupt OUT endpoint accepts
  1025-byte reports and completes them in `0 ms`.
- When the firmware **rejects** a report — wrong order, transfer already open,
  wrong length — the endpoint halts.
- Every later write then fails, in this process and in new ones, and the vendor
  app fails too. That is why the deck looks "dead but backlit" and why Stream
  Station behaves "as if it were closed".
- Only a real power cycle clears it.

**Known triggers:** sending `STP` (flush) *before* the payload chunk, and
sending `CRT|VER` — the firmware serves the version from a static identity
report and rejects a written `VER`. The firmware sees a commit or a request it
cannot serve, rejects it, and the endpoint halts. The next write returns `0x1F`,
then `0xE5` forever.

**Idleness is not required** to reach the failure state: after a verified 60 s
cycle with all keys blank, a probe that settled 10 s before its first write
still got an instant refusal, so the endpoint was already dead **before this host
sent anything**.

### 5.5 Probe inventory

Run the tool **first**, then replug; every one of these waits for the device.

| Probe | Question |
|---|---|
| `quiet_probe` | does the deck die with zero host writes? |
| `reboot_probe` | is the deck healthy after its boot re-enumeration? |
| `hotplug_probe` | how long is stage 1, measured from the first appearance? |
| `handshake_probe` | does the bare `HAN` handshake hold the endpoint? |
| `keepalive_probe` | does `CRT|CONNECT` hold it? |
| `read_in_probe` | does draining IN change anything? |
| `reopen_probe` | does re-opening the handle heal it? |
| `resume_probe` | is it USB selective suspend? |
| `idle_halt_probe` | is there an idle threshold? |

<!-- -->

---

## 6. Code rules that prevent a repeat

These are requirements, not suggestions:

1. **Never write `CRT|VER`.** The version is a static identity report
   available from `GET_REPORT` with no command. Writing it halts the endpoint.
2. **Validate the whole sequence before sending one byte.** `BAT` and its
   chunks and exactly one trailing `STP` must be built and checked atomically.
   A `STP` with no preceding `BAT` → chunks is a bug, not a user error.
3. **Never send `STP` unless the matching `BAT` was sent and all its chunks
   were written.** `Device::flush` is a no-op when nothing is queued.
4. **On any write error, mark the device wedged and stop.** No retry loop: each
   rejected report extends the halt. `Ss550Error::TransportStalled` names the
   recovery instead of a generic HID error.
5. **Never leave brightness at 0.** `initialize()` used to send `LIG 0` and
   rely on a later `flush()` to restore it; if the flush never landed, the deck
   sat dark with no signal. It no longer touches brightness at all.
6. **Preflight the transport** before a paint session: one harmless write that
   must complete, and a distinct error if it does not.
7. **Never keep a handle across the deck's boot re-enumeration.** `Device::connect`
   enforces this: it waits for [`SETTLE_TIME`] (3 s) of **continuous** presence
   before opening the handle it will use, and returns `DeviceNotFound` fast if
   the deck was never seen. `present → absent → present` alone is **not**
   sufficient — that exact pattern was measured landing on an intermediate
   appearance that lasted under 502 ms. Never open a handle yourself; use the
   driver.
8. **Feed the deck a heartbeat.** It resets itself after ~68 s of host silence,
   and the reset kills the handle. Call `Device::keepalive` once per main-loop
   frame; it is throttled internally to 2 s and never fires during an upload. An
   idle app that skips this will watch the deck die for no visible reason.

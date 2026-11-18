# PawnIO module: AMDFamily17

`AMDFamily17.bin` is a **signed PawnIO module** — bytecode for the small virtual
machine inside PawnIO's driver — and it exposes the primitive this app needs to
read a Ryzen's temperature: `ioctl_read_smn`, which reads one System Management
Network register.

## Where it comes from

- Upstream: `LibreHardwareMonitorLib/Resources/PawnIo/AMDFamily17.bin` in
  [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor),
  which takes its modules from
  [PawnIO.Modules 0.2.11](https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11).
- SHA-256 of the vendored copy:
  `DAE74615761B78BDF064DFB3E136252DDCC6FC727D88F14738D0E5800D427A91`
- Size: 10 652 bytes.

## Licence

LGPL-2.1 — see [`COPYING`](COPYING), which is the copy shipped upstream beside
these modules. PawnIO's **driver** is GPL-2.0, but it is not distributed here: the
user installs PawnIO (LibreHardwareMonitor's first run does it), and this crate
only loads the driver's own `PawnIOLib.dll` at run time.

The corresponding source of the module lives in
[PawnIO.Modules](https://github.com/namazso/PawnIO.Modules), at the tag above;
`README.upstream` is the note that accompanied the upstream copy.

## What this app does with it

`crates/ss550-sensors/src/pawnio.rs` loads the blob once and calls
`ioctl_read_smn` with a single register address. It never calls a write
primitive, so this app cannot change a processor's clocks, voltages or limits.

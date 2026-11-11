# SS550 protocol fuzzer

`cargo fuzz` harness for the decoder in [`ss550-protocol`](../crates/ss550-protocol).
It exists for one reason: **the decoder must not panic on arbitrary input.**

The deck is a physical USB device. A malformed report that makes the decoder
panic takes the host process down with it, and a decoder that trusts the
device is exactly what broken — or hostile — firmware can attack. `ss550-protocol`
is the one piece of code that has to survive whatever bytes arrive on the
interrupt IN endpoint.

The target asserts **nothing** about the result of a decode. Rejecting a
malformed report is a correct outcome; panicking is not. "No panic" is the only
pass condition.

## What the target covers

`fuzz_targets/decode_input_report.rs` feeds arbitrary bytes to every decoder
entry point of `ss550-protocol`:

| Call | Inputs |
|---|---|
| `parse_input_report` | the raw bytes; **every prefix** of them; the same bytes wrapped in a valid `ACK`/`OK` envelope; the same bytes in an oversized (1025-byte) buffer |
| `is_key_edge` | the same four sets |
| `read_index_to_firmware_key` | every value the fuzzer can produce |

The four shapes exist for four different reasons:

- **Raw bytes** are what a hostile or broken device actually sends.
- **Every prefix** exposes an off-by-one in the length guard: if the decoder
  ever read `report[10]` without checking the length first, a 10-byte input
  would be the one that breaks it.
- **A valid envelope** matters because random bytes never spell `ACK` at the
  front. Without it the fuzzer only ever reaches the envelope check, and the
  key-index and state fields are never reached with adversarial values.
- **The oversized buffer** catches a decoder that assumed the exact 512-byte
  report size instead of the minimum length it needs.

The empty input is covered by the prefix sweep (`0..=data.len()` includes
length 0).

## Toolchain

The harness needs **nightly**, because `cargo fuzz` compiles it with
`-Zsanitizer=address` and sanitizer-coverage instrumentation. This is a
development-only requirement: the fuzz crate is outside the repository
workspace, it is never built by `cargo build --workspace`, and the workspace
MSRV stays at **1.87**.

```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly   # cargo fuzz builds std with --build-std
cargo install cargo-fuzz
```

`cargo-fuzz` uses the toolchain rustup currently defaults to, and it does
**not** pick one itself. Select nightly per invocation with the environment
variable:

```bash
RUSTUP_TOOLCHAIN=nightly cargo fuzz run decode_input_report
```

On PowerShell:

```powershell
$env:RUSTUP_TOOLCHAIN = "nightly"
cargo fuzz run decode_input_report
```

A directory override does **not** work here: `rustup` selects the toolchain
from the current working directory, and `cargo fuzz` invokes
`cargo build --manifest-path fuzz/Cargo.toml` from the repository root, so
`rustup override set --path fuzz nightly` is ignored and the build fails with
`error: the option 'Z' is only accepted on the nightly compiler`.

## Running it

From the repository root:

```bash
RUSTUP_TOOLCHAIN=nightly cargo fuzz run decode_input_report
```

That runs until it finds a crash. For a bounded session:

```bash
RUSTUP_TOOLCHAIN=nightly cargo fuzz run decode_input_report -- -max_total_time=60 -print_final_stats=1
```

A clean run ends with something like this — the session recorded for the
milestone, so the number can be checked against the changelog:

```text
Done 142297 runs in 61 second(s)
stat::number_of_executed_units: 142297
stat::average_exec_per_sec:     2332
stat::new_units_added:          3
stat::slowest_unit_time_sec:    0
stat::peak_rss_mb:              542
```

and exit code `0`. A crash would land in `fuzz/artifacts/decode_input_report/`
and the run would exit non-zero.

The corpus lives in `fuzz/corpus/decode_input_report/` and is kept between
runs, so a later session starts from what an earlier one learned. Both the
corpus and the artifacts are machine-local and are **not** committed.

### Windows and MSVC

On `x86_64-pc-windows-msvc` the AddressSanitizer runtime is **not** part of
the Rust toolchain. Rust links against `clang_rt.asan_dynamic-x86_64.dll`,
which ships with the Visual Studio Build Tools, and the harness dies with
`STATUS_DLL_NOT_FOUND` unless that directory is on `PATH`:

```powershell
$env:PATH = "$env:PATH;C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64"
$env:RUSTUP_TOOLCHAIN = "nightly"
cargo fuzz run decode_input_report -- -max_total_time=60 -print_final_stats=1
```

Adjust the MSVC version to whatever `BuildTools` you have installed. The
directory also holds `link.exe`, so adding it to `PATH` is useful anyway.

`--sanitizer none` is **not** an alternative on this platform: with the
sanitizer switched off, `cargo fuzz` still compiles the harness with
sanitizer-coverage instrumentation, whose section-boundary symbols
(`__start___sancov_pcs`, `__stop___sancov_pcs`) are ELF/Mach-O linker
features that do not exist in the Windows COFF format. The build then fails
with `LNK2019: unresolved external symbol __stop___sancov_cntrs`. The
AddressSanitizer runtime happens to export those symbols, which is why the
default sanitizer is the only configuration that links on Windows.

On Linux and macOS the plain command works with no extra setup.

## Layout

```
fuzz/
  Cargo.toml                       # own workspace root, depends on ss550-protocol by path
  Cargo.lock                       # committed, so the harness keeps resolving the same versions
  .gitignore                       # target/, corpus/, artifacts/
  README.md
  fuzz_targets/decode_input_report.rs
```

`fuzz/Cargo.toml` carries its own empty `[workspace]` table on purpose.
`cargo fuzz` requires a standalone package, and keeping it out of the
repository workspace means the fuzz lock file and the nightly-only
dependencies never touch the three shipped lock files
(`Cargo.lock`, `crates/ss550-gui/Cargo.lock`, `crates/ss550-tray/Cargo.lock`).

## Checking the harness itself

The target is not part of `cargo test --workspace`, so lint it on its own:

```bash
cargo clippy --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path fuzz/Cargo.toml --check
```

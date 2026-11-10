# ss550-probe

One-shot live-device probe harness for the Redragon StreamCraft SS550
(VID `0x0200` / PID `0x3000`, 15 LCD buttons). Fase 0 of the `streamcraft-rs` plan.

## Usage

```sh
cargo run -p ss550-probe          # P2 (G1): enumerate + report lengths (read-only)
cargo run -p ss550-probe -- --han # P1B (G2): P2 + one verified HAN write + reads
```

The only write this harness ever performs is the single verified `HAN`
handshake pack (`addHandshakePack 0x180015e00`) documented in
`docs/protocol/probe-p1b.md`. Unknown opcodes are never sent.

## Specs

- `docs/protocol/probe-p2.md` (P2 / G1)
- `docs/protocol/probe-p1b.md` (P1B / G2)

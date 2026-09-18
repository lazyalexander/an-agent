# an-agent

<p align="center">
  <img src="assets/icon.png" width="128" alt="an-agent icon">
</p>

A research-oriented, audit-first agent framework: distributed by assumption, reproducible by construction.

The core bet: an append-only event tape (memstream) is the single source of truth. Every action an agent takes — speaking, calling a tool, calling a model — passes an admission layer and lands on the tape as intent + effect. From the tape alone you can audit what happened, and replay a run with the tape standing in for the non-deterministic parts (the model, the world).

## Layout

- `crates/an-agent` — the Rust kernel (tape, admission, agent identity, tool admission) plus runnable probes under `tests/`. See its README for the module map and invariants.
- `packages/causal-web` — zero-dependency tape viewer: drop in a `.jsonl` tape, see the causal threads, get the tape validated in-page.
- `packages/lean-probe` — tiny Lake package used by the Lean 4 compatibility probe (no Mathlib).
- `config/` — model endpoint configuration.

## Status

Early research, built in thin slices. The kernel compiles clean under a pinned toolchain with deny-level lints. The ReAct loop and HTTP client deliberately live in `tests/` as probe scaffolding — not in the kernel — until their shape stabilizes. Design contracts are being drafted alongside the code and will be published when the slices they govern land.

## Develop

```sh
cargo check --all-targets
cargo clippy --all-targets   # deny-level lints; disallowed-methods enforce the determinism seam
cargo test
```

Live probes (need network + `MODEL_API_KEY`, skipped by default):

```sh
cargo test -p an-agent --test react_live -- --ignored --nocapture
cargo test -p an-agent --test web_search -- --ignored --nocapture
```

The toolchain is pinned via `rust-toolchain.toml`. Upgrade one minor at a time; fix new lints in the same commit.

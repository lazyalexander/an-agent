# an-agent

<p align="center">
  <img src="assets/icon.png" width="128" alt="an-agent icon">
</p>

A research-oriented, audit-first agent framework: distributed by assumption, reproducible by construction.

The core bet: an append-only event tape (memstream) is the single source of truth. Every action an agent takes — speaking, calling a tool, calling a model — passes an admission layer and lands on the tape as intent + effect. From the tape alone you can audit what happened, and replay a run with the tape standing in for the non-deterministic parts (the model, the world).

## Layout

- `crates/an-agent-core` — the kernel: tape, admission, identity, `AgentControl`, seats, and the recorder file workspace.
- `crates/an-agent-factory` — bash registration and `BuiltAgent` construction. `spawn` returns an `Agent` and is not the host face.
- `crates/an-agent-context` — per-session context assembly. The shared pool is not in this crate.
- `crates/an-agent-spool` — capability spools: YAML descriptor admission and the append-only spool registry.
- `crates/an-agent-workspace` — content-addressed store. It is not the recorder workspace.
- `packages/causal-web` — zero-dependency tape viewer: drop in a `.jsonl` tape, see the causal threads, get the tape validated in-page.
- `packages/lean-probe` — tiny Lake package from a Lean 4 compatibility experiment (no Mathlib).
- `config/` — model endpoint configuration.

## Status

Early research, built in thin slices. The kernel compiles clean under a pinned toolchain with deny-level lints. The probe harness (model clients, the ReAct loop, link probes) was retired once the host face (`AgentControl`) landed; what survived the retirement lives in `an-agent-spool` as the rhai policy constructor. Design contracts are being drafted alongside the code and will be published when the slices they govern land.

## Develop

```sh
cargo check --all-targets
cargo clippy --all-targets   # deny-level lints; disallowed-methods enforce the determinism seam
cargo test
```

The toolchain is pinned via `rust-toolchain.toml`. Upgrade one minor at a time; fix new lints in the same commit.

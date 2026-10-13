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
- `crates/an-agent-semantic` — transport-neutral claims, concrete attempts, version-bound assessments, grants, and execution receipts. It owns no scheduling or interruption. Core execution boundaries retain evidence and return supervisor requests for blocked operations.
- `crates/an-agent-supervision` — transport-neutral live review state and exact one-shot approvals. It owns no tape, execution, or scheduling; core adapts trusted execution boundaries to it.
- `crates/an-agent-spool` — capability spools: YAML descriptor admission and the append-only spool registry.
- `crates/an-agent-workspace` — content-addressed store. It is not the recorder workspace.
- `packages/causal-web` — zero-dependency tape viewer: drop in a `.jsonl` tape, see the causal threads, get the tape validated in-page.
- `packages/lean-probe` — tiny Lake package from a Lean 4 compatibility experiment (no Mathlib).
- `config/` — model endpoint configuration.

## Status

Early research, built in thin slices. The kernel compiles clean under a pinned toolchain with deny-level lints. The probe harness (model clients, the ReAct loop, link probes) was retired once the host face (`AgentControl`) landed; what survived the retirement lives in `an-agent-spool` as the rhai policy constructor. Design contracts are being drafted alongside the code and will be published when the slices they govern land.

Supervision is host-driven: bind a live supervisor with `set_supervisor`, consume
`NeedsReview` outcomes or `ReviewRequired` tool errors, and let the supervisor
dispose through `resolve_review`. Waiting frees the beat and does not unmount a
body. `Ask` tools require one exact approval; `Deny` and absent capabilities are
not overridden. Responses do not retry work or contact third parties. Approved
calls are revalidated and consumed before execution; pending approvals are not
restored from serialized tape records.

Reviews bind live agent and mount instances, authority epochs, and workspace
generations. Replacement or retirement closes affected reviews; restoring old
content does not restore old approval. Revocation survives audit-write failure,
while approval activation requires successful evidence delivery.

This is a process-local supervision slice, not a complete sandbox or replay
engine. OS enforcement, durable review recovery, host-document version fences,
and external-effect idempotency remain separate work. Already admitted effects
are not rolled back by later revocation.

## Develop

```sh
cargo check --all-targets
cargo clippy --all-targets   # deny-level lints; disallowed-methods enforce the determinism seam
cargo test
```

The toolchain is pinned via `rust-toolchain.toml`. Upgrade one minor at a time; fix new lints in the same commit.

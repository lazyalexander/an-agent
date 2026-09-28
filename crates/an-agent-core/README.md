# an-agent-core

The kernel. Tool constructors, context assembly, and the workplace CAS store live in `crates/an-agent`.

## Modules

| module | role |
|---|---|
| `memstream` | The tape. Append-only JSONL; events carry kind, refs (causal edges), act envelopes, and the producing card's hash. Readers absorb schema drift (serde defaults + `skip_serializing_if`); history is never rewritten. |
| `act` | Admission. Every effect-ful action passes here: a sentence (permit × file × memory facets) is decided, intent is taped before execution, effect after. Resource mnemonics live here so a sentence can name one. `ActKind` is intent-domain vocabulary only; the channel (tool vs model) rides on `ActEnvelope.tool`. |
| `principal` | Agent identity. An `AgentCard` = model spec + prompt + tool grants + kernel ref — immutable, content-addressed, versioned by supersession, never mutated. Cards hold no memory and no keys. `factory::build_with` takes the caller's tool constructors; this crate registers none. |
| `instance` | Loop runtime: private `Session` (tape/files/scratch), live `Agent`, registration `Tree`, bounded turn `Pool`, steward, and the steward workspace (`wp`, one sub-workspace per worker). `spawn_with` takes the same constructor list as the factory. |
| `det_seam` | The determinism seam — the only place allowed to touch OS time and entropy. Enforced by `clippy.toml` disallowed-methods, not by reviewer vigilance. |

## Invariants (what a review should police)

1. The tape is append-only. New semantics arrive as new fields or new kinds; old lines must stay readable.
2. Model calls and tool calls are both `invoke` acts; the tape records thin traces (hashes, token usage), never full payloads.
3. Tools are cognition-free effectors. Anything containing an LLM call is an agent, reached by delegation — never a tool.
4. The kernel never depends on `tests/` or on the composition crate.
5. No OS time/entropy outside `det_seam`; no `unwrap`/`expect` outside tests.

## Conventions

One folder = one proto-package: every module directory carries a thin README (purpose, admission rule, dependency direction). Rule-level text only — implementation details drift.

Follow the repo-level discipline: `cargo check → clippy → test`, batch edits before compiling, no `cargo clean`. Dependencies are centralized in the workspace root; add nothing without a reason.

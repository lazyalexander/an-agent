# an-agent (kernel crate)

The kernel: a handful of mechanisms and the invariants between them. Everything a reviewer or a collaborating agent needs is the module map plus the invariant list below.

## Modules

| module | role |
|---|---|
| `memstream` | The tape. Append-only JSONL; events carry kind, refs (causal edges), act envelopes, and the producing card's hash. Readers absorb schema drift (serde defaults + `skip_serializing_if`); history is never rewritten. |
| `act` | Admission. Every effect-ful action passes here: a sentence (permit × file × memory facets) is decided, intent is taped before execution, effect after. `ActKind` is intent-domain vocabulary only; the channel (tool vs model) rides on `ActEnvelope.tool`. |
| `principal` | Agent identity. An `AgentCard` = model spec + prompt + tool grants + kernel ref — immutable, content-addressed, versioned by supersession, never mutated. Cards hold no memory and no keys. |
| `tools` | Tool implementations (today: `bash`) and strict YAML descriptor admission (`descriptor`): parse-or-reject, unknown fields fatal, effect faces fully explicit. |
| `workplace` | Workspace resources and addressing. |
| `det_seam` | The determinism seam — the only place allowed to touch OS time and entropy. Enforced by `clippy.toml` disallowed-methods, not by reviewer vigilance. |

## Invariants (what a review should police)

1. The tape is append-only. New semantics arrive as new fields or new kinds; old lines must stay readable.
2. Model calls and tool calls are both `invoke` acts; the tape records thin traces (hashes, token usage), never full payloads.
3. Tools are cognition-free effectors. Anything containing an LLM call is an agent, reached by delegation — never a tool.
4. The kernel never depends on `tests/`. The ReAct loop, the HTTP client, and the rhai shim are probe scaffolding there, expected to be replaced.
5. No OS time/entropy outside `det_seam`; no `unwrap`/`expect` outside tests.

## Probes (`tests/`)

- `agent_loop` — offline loop semantics: observation→action refs linkage, forbidden permits never execute.
- `web_search` — a rhai-constructed tool driven by a YAML descriptor (`fixtures/web_search.yaml`). The offline half proves the declared effect *is* the host-function wiring: declare `net: none` and `http_get_json` ceases to exist for the script.
- `react_live` — live model run (search + deliberate remember) asserting the tape alone reconstructs the run, including per-call model intents/effects and token usage.
- `lean_tool` — Lean 4 as a subprocess `Tool` (`#[ignore]`, needs elan). The test script is the control flow.
- `lean_math` — rhai continuation script drives `lean_check` over Init arithmetic identities (`#[ignore]`, needs elan).

Probes self-clean their temp dirs; setting `AN_AGENT_PROBE_DIR` keeps the tape at a chosen location instead.

## Conventions

One folder = one proto-package: every module directory carries a thin README (purpose, admission rule, dependency direction). When a folder gains an independent consumer, it graduates to a crate and its README is the draft crate README. Rule-level text only — implementation details drift.

Follow the repo-level discipline: `cargo check → clippy → test`, batch edits before compiling, no `cargo clean`. Dependencies are centralized in the workspace root; add nothing without a reason.

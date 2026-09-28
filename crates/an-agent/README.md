# an-agent (composition crate)

Re-exports [`an-agent-core`](../an-agent-core/README.md) and mounts what is not the kernel: context assembly, tool constructors, and the workplace CAS store. Probes live in `tests/`.

## Modules

| module | role |
|---|---|
| `context` | Session context: compressed markdown that cites tape events, plus a rebuildable sidecar index. Re-exported from `instance`. |
| `factory` | Registers the `bash` constructor and calls the kernel's `build_with`. `principal::factory::build` and `instance::spawn` keep that list. |
| `tools` | Tool implementations (today: `bash`) and strict YAML descriptor admission (`descriptor`): parse-or-reject, unknown fields fatal, effect faces fully explicit. |
| `workplace` | CAS store. `Resource` and `ResourceKind` are re-exported from the kernel. |

Kernel modules (`memstream`, `act`, `principal`, `instance`, `det_seam`) are re-exported from `an-agent-core`. See that crate's README for the invariant list.

## Probes (`tests/`)

- `agent_loop` — offline loop semantics: observation→action refs linkage, deny permits never execute.
- `web_search` — a rhai-constructed tool driven by a YAML descriptor (`fixtures/web_search.yaml`). The offline half proves the declared effect *is* the host-function wiring: declare `net: none` and `http_get_json` ceases to exist for the script.
- `react_live` — live model run (search + deliberate remember) asserting the tape alone reconstructs the run, including per-call model intents/effects and token usage.
- `lean_tool` — Lean 4 as a subprocess `Tool` (`#[ignore]`, needs elan). The test script is the control flow.
- `lean_math` — rhai continuation script drives `lean_check` over Init arithmetic identities (`#[ignore]`, needs elan).

Probes self-clean their temp dirs; setting `AN_AGENT_PROBE_DIR` keeps the tape at a chosen location instead.

## Conventions

One folder = one proto-package: every module directory carries a thin README (purpose, admission rule, dependency direction). When a folder gains an independent consumer, it graduates to a crate and its README is the draft crate README. Rule-level text only — implementation details drift.

Follow the repo-level discipline: `cargo check → clippy → test`, batch edits before compiling, no `cargo clean`. Dependencies are centralized in the workspace root; add nothing without a reason.

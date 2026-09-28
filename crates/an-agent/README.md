# an-agent (composition crate)

Re-exports the kernel, [`an-agent-tool`](../an-agent-tool/README.md), and [`an-agent-workspace`](../an-agent-workspace/README.md). Context assembly and the bash registration live here. Probes live in `tests/`.

## Modules

| module | role |
|---|---|
| `context` | Session context: compressed markdown that cites tape events, plus a rebuildable sidecar index. Re-exported from `runtime`. |
| `factory` | Registers the `bash` constructor and calls the kernel's `build_with`. `principal::factory::build` and `runtime::spawn` keep that list. The factory returns a `BuiltAgent`. |
| `tools` | Re-export of `an-agent-tool`. |
| `workspace` | Re-export of `an-agent-workspace`, the CAS store. `Resource` and `ResourceKind` still come from the kernel. |

Kernel modules (`memstream`, `act`, `principal`, `runtime`, `det_seam`) are re-exported from `an-agent-core`. See that crate's README for the invariant list.

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

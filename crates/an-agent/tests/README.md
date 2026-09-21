# tests — probes, not kernel

Everything here is probe scaffolding. The ReAct loop, the chat-completions HTTP client, and the rhai shim live in `support/` and are expected to be replaced; the kernel must never depend on this directory (the compiler enforces it — src cannot import from tests/).

**Files:**

- `agent_loop.rs` — offline loop semantics: observation→action refs, deny permits never execute, model calls taped as invoke acts.
- `web_search.rs` — rhai-constructed tool from `fixtures/web_search.yaml`; the offline half proves the declared effect *is* the host-function wiring.
- `react_live.rs` — live model run (search + deliberate remember); asserts the tape alone reconstructs the run, including per-call invoke intents/effects and token usage.
- `lean_tool.rs` — Lean 4 CLI behind `Tool` (`#[ignore = "needs lean4"]`). Control flow is the test script: theorem, type error, `lake build` of `packages/lean-probe`.
- `lean_math.rs` — rhai continuation script yields Lean snippets (`fixtures/lean_math.rhai`); host admits `lean_check`. Init identities plus one type error (`#[ignore = "needs lean4"]`).
- `ts_tool.rs` / `bench_tool.rs` — bun subprocess adapter and constructor-overhead bench.
- `support/` — shared probe scaffolding (loop, client, rhai, ts, lean, temp dirs).
- `fixtures/` — YAML descriptors under test.

**Hygiene:** probes self-clean temp dirs; set `AN_AGENT_PROBE_DIR` to keep a tape. Live tests are `#[ignore]`d (`react_live` / `web_search` need network; `lean_tool` / `lean_math` need elan+Lean 4; `bench_tool` is a bench).

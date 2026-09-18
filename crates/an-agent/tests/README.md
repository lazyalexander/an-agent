# tests — probes, not kernel

Everything here is probe scaffolding. The ReAct loop, the chat-completions HTTP client, and the rhai shim live in `support/` and are expected to be replaced; the kernel must never depend on this directory (the compiler enforces it — src cannot import from tests/).

**Files:**

- `agent_loop.rs` — offline loop semantics: observation→action refs, forbidden permits never execute, model calls taped as invoke acts.
- `web_search.rs` — rhai-constructed tool from `fixtures/web_search.yaml`; the offline half proves the declared effect *is* the host-function wiring.
- `react_live.rs` — live model run (search + deliberate remember); asserts the tape alone reconstructs the run, including per-call invoke intents/effects and token usage.
- `policy_loop.rs` — policy-as-script control flow: a rhai policy (`fixtures/echo_policy.yaml`) is mounted on tape, then drives the loop one continuation at a time (invoke_model / invoke_tool / utter / halt). The host owns the loop and admission; the script owns the policy.
- `support/` — shared probe scaffolding (loop, client, rhai tool, temp dirs).
- `fixtures/` — YAML descriptors under test.

**Hygiene:** probes self-clean temp dirs; set `AN_AGENT_PROBE_DIR` to keep a tape. Live tests are `#[ignore]`d and need network (and `MODEL_API_KEY` for react_live).

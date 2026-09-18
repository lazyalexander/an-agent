# act

Admission: every effect-ful action passes through here. A sentence (permit × file × memory facets) is decided, intent is taped before execution, effect after.

**Admission rule:** the permission vocabulary (`tag`, `sentence`), the envelope, act-wrapping (`run_tool_act`), and the scoped tool registry (`ToolRegistry` — spatial admission: which tools exist, with RAII unregister). No tool implementations, no model code — both are out-calls this layer surrounds.

**Grant semantics (today):** a grant's tag is an audit label, not a gate — only `Permit::Forbidden` blocks execution in `run_tool_act`; `Ask` is taped but runs as `Go`. Real allow/deny/ask enforcement arrives with the delegation-chain + Ask-grant slice.

**Vocabulary rule:** `ActKind` is intent domains only (utterance, invoke, remember, …). The channel rides on `ActEnvelope.tool`: `Some(name)` = via tool, `None` = model-side or direct. Generic tool calls and model calls share `invoke`.

**Dependencies:** `memstream` (to tape), `workplace` (resources). Never: `principal`, `tools`.

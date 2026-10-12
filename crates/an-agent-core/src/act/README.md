# act

Admission: every effect-ful action passes through here. Intent is taped before execution, effect after. An act carries a permit. The effect is projected from the tool tag. A spawn charter is a separate ceiling (tool tag plus signal) and is not stored on the act.

**Admission rule:** the permission vocabulary (`Permit`, `ToolTag`, `Charter`), the envelope, act-wrapping (`run_tool_act`), and the scoped tool registry (`ToolRegistry` — spatial admission: which tools exist, with RAII unregister). No tool implementations, no model code — both are out-calls this layer surrounds.

**Grant semantics:** `Deny` blocks; the public `run_tool_act` refuses `Ask` before execution. Only the trusted control boundary can consume a bound supervisor's one-shot approval and enter the reviewed Ask path. Approval does not widen the card's file or memory faces. File paths remain recorded scope, not OS sandbox enforcement.

**Vocabulary rule:** `ActKind` is intent domains only (utterance, invoke, remember, …). The channel rides on `ActEnvelope.tool`: `Some(name)` = via tool, `None` = model-side or direct. Generic tool calls and model calls share `invoke`.

**Dependencies:** `memstream` (to tape). Resource mnemonics live in this module. Never: `principal`, `an-agent-spool`, the workspace CAS store.

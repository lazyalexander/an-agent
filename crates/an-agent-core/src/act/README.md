# act

Admission: every effect-ful action passes through here. Intent is taped before execution, effect after. An act carries a permit. The effect is projected from the tool tag. A spawn charter is a separate ceiling (tool tag plus signal) and is not stored on the act.

**Admission rule:** the permission vocabulary (`Permit`, `ToolTag`, `Charter`), the envelope, act-wrapping (`run_tool_act`), and the scoped tool registry (`ToolRegistry` — spatial admission: which tools exist, with RAII unregister). No tool implementations, no model code — both are out-calls this layer surrounds.

**Grant semantics:** `Deny` blocks; the public `run_tool_act` refuses `Ask` before execution. Only the trusted control boundary can consume a bound supervisor's one-shot approval and enter the reviewed Ask path. Approval does not widen the card's file or memory faces. File paths remain recorded scope, not OS sandbox enforcement.

**Vocabulary rule:** `ActKind` is intent domains only (utterance, invoke, remember, …). The channel rides on `ActEnvelope.tool`: `Some(name)` = via tool, `None` = model-side or direct. Generic tool calls and model calls share `invoke`.

**Dependencies:** `memstream` (to tape). Resource mnemonics live in this module. Never: `principal`, `an-agent-spool`, the workspace CAS store.

**Tool payload boundary:** new tool actions, observations, and remember clips carry
`tool_payload_v1` references (SHA-256 and byte length), not argument/result bodies.
Bodies live in the session's private `blobs/` directory; readers verify identity
and retain legacy inline compatibility. Tool names and call IDs are limited to
256 bytes, and arguments/results to 1 MiB. Oversized calls fail before review,
snapshot storage, or execution; oversized
results leave a rejection observation and return an error, because effects may
already have happened. This is an admission/storage bound, not a limit on memory
allocated inside a tool. Live bounded tool messages still contain the result.
Blobs and derived context are sensitive; this change does not erase historical
inline logs or redact private storage. A tape backup needs its referenced blobs.

Explicitly selected clips must resolve successfully before a model request is
recorded or sent. Missing or corrupt bodies reject the invocation; unavailable
markers are only historical previews. Derived tool context includes the tool
name from the event envelope, while the raw payload API still returns the body
alone. Context assembly and cuts preserve this identity without inlining bodies
back into the tape.

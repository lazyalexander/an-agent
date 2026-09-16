# act

Admission: every effect-ful action passes through here. A sentence (permit × file × memory facets) is decided, intent is taped before execution, effect after.

**Admission rule:** the permission vocabulary (`tag`, `sentence`), the envelope, and act-wrapping (`run_tool_act`). No tool implementations, no model code — both are out-calls this layer surrounds.

**Vocabulary rule:** `ActKind` is intent domains only (utterance, invoke, remember, …). The channel rides on `ActEnvelope.tool`: `Some(name)` = via tool, `None` = model-side or direct. Generic tool calls and model calls share `invoke`.

**Dependencies:** `memstream` (to tape), `workplace` (resources). Never: `principal`, `tools`.

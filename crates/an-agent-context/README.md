# an-agent-context

Per-session context: compressed markdown that cites tape events, plus a rebuildable sidecar index.

**Admission rule:** a projection of one agent’s private session tape. The tape stays the record. This crate writes `ctx/` only. `AgentControl` appends `compress` and `context`. A piece may cite an id outside that tape (`Piece::outside`). Continue assembly sets that for the current workspace config and env. Independent assembly does not. A shared pool is not in this crate.

Use `assemble_context`, `cut_context`, and `summarize_context` with a mounted agent ID. The previous `*_thread` names call the same operations.

**Dependencies:** `an-agent-core` (`memstream`, `control`). Never: the recorder workspace, tool constructors.

Audit/semantic records are evidence, not implicit context. New cuts and recent
assembly exclude them; cached events and old segments citing them are refused
without deleting history. This is direct-source exclusion, not a general
provenance-withdrawal graph or validation of arbitrary imported summaries.

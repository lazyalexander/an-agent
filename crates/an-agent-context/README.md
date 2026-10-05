# an-agent-context

Per-session context: compressed markdown that cites tape events, plus a rebuildable sidecar index.

**Admission rule:** a projection of one thread tape. The tape stays the record. This crate writes `ctx/` only. `AgentControl` appends `compress` and `context`. A piece may cite an id off the thread (`Piece::outside`). Continue assembly sets that for the current workspace config and env. Independent assembly does not. A shared pool is not in this crate.

**Dependencies:** `an-agent-core` (`memstream`, `control`). Never: the recorder workspace, tool constructors.

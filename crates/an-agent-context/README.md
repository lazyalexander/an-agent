# an-agent-context

Per-session context: compressed markdown that cites tape events, plus a rebuildable sidecar index.

**Admission rule:** a projection of one session tape. The tape stays the record. A shared pool is not in this crate.

**Dependencies:** `an-agent-core` (`memstream`). Never: spawn, the steward workspace, tool constructors.

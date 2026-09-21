# instance

Runtime topology the loop needs: a private `Session`, a live `Agent`, a registration `Tree`, a bounded turn `Pool`, and one `Steward`.

**Admission rule:** construction and spatial presence. Session is the agent's private domain (tape, files, scratch). The tree says which agents exist right now; dropping a seat unregisters the subtree. The pool says how many turns may run. Only the steward may spawn. Its world-facing grants are Deny; spawn, mail, and link are verbs on `Steward`, not tools. A view is a named list of link event ids already on the steward tape. A worker sub-workplace is an append-only path log (`subwp/`) in that session; the steward only stores its hash.

**Dependencies:** `principal` (card + factory), `memstream`, `act` (ActCtx, Tool). Never: `workplace`, `tests/`.

**Not here:** the ReAct loop, HTTP, rhai, TUI queue chrome, workplace CAS.

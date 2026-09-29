# runtime

One process, one runtime: a private `Session`, a live `Agent`, a registration `Tree`, a bounded turn `Pool`, and one `Recorder`.

**Admission rule:** construction and spatial presence. Session is the agent's private domain (tape, files, scratch). The tree says which agents exist right now; dropping a seat unregisters the subtree. The pool says how many turns may run. Only the recorder may spawn. Its world-facing grants are Deny; spawn, mail, and link are verbs on `Recorder`, not tools. A view is a named list of link event ids already on the recorder tape. The recorder owns one workspace. Each worker has one sub-workspace on that tree. Closing it publishes a view, or asks when two true names claim one path. The recorder does not write file bytes.

**Dependencies:** `principal` (card + factory), `memstream`, `act` (ActCtx, Tool). Never: the workspace CAS store, `tests/`.

**Not here:** context assembly, tool constructors, the ReAct loop, HTTP, rhai, TUI queue chrome, the CAS store.

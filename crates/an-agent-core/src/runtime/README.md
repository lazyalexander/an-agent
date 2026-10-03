# runtime

Directory name, not a concept. Pieces of the process-local loop: a private `Session`, a live `Agent`, a registration `Tree`, a bounded turn `Pool`, one `Recorder`, and the host face `AgentControl`. There is no runtime type. `AgentControl` writes the tape.

**Admission rule:** construction and spatial presence. `AgentControl` is the host face: open a thread, advance, cancel, finish, send, and run a granted tool. One thread, one writer, one tape, born at `open_thread`. Its pool runs one beat at a time. It has no workspace and no spool. Session is the agent's private domain (tape, files, scratch). The tree says which agents exist right now; dropping a seat unregisters the subtree. The pool says how many turns may run. Only the recorder may spawn workers onto the file workspace. Its world-facing grants are Deny; spawn, mail, and link on that side are verbs on `Recorder`, not tools. A view is a named list of link event ids already on the recorder tape. The recorder owns one file workspace (`Wp`). Each worker has one sub-workspace on that tree. Closing it publishes a view, or asks when two true names claim one path. The recorder does not write file bytes. `spawn_with` stays the card-and-tape constructor. It is not the host face.

**Dependencies:** `principal` (card + factory), `memstream`, `act` (ActCtx, Tool). Never: the workspace CAS store, `tests/`.

**Not here:** context assembly, tool constructors, the ReAct loop, HTTP, rhai, TUI queue chrome, the CAS store.

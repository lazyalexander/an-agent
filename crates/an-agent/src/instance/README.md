# instance

Runtime topology the loop needs: a private `Session`, a live `Agent`, a registration `Tree`, a bounded turn `Pool`.

**Admission rule:** construction and spatial presence. Session is the agent's private domain (tape, files, scratch). The tree says which agents exist right now; dropping a seat unregisters the subtree. The pool says how many turns may run. Workplace mount is not in this module.

**Dependencies:** `principal` (card + factory), `memstream`, `act` (ActCtx, Tool). Never: `workplace`, `tests/`.

**Not here:** the ReAct loop, HTTP, rhai — those stay probe scaffolding until the loop shape stabilizes.

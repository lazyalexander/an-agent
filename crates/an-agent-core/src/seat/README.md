# seat

Who is mounted in this process, and which one beat may run.

**Admission rule:** the tree is presence. Dropping a seat drops its subtree. The pool admits one turn at a time and does not run guest scripts. `AgentControl` and the recorder both use this. Neither owns it.

**Dependencies:** `agent`. Never: `recorder`, `control`, the workspace CAS store.

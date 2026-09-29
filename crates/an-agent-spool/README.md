# an-agent-spool

Internal capabilities, wound onto spools. The kernel does not depend on this crate.

**Vocabulary discipline:** a *spool* is an internal capability — it never touches the outside world directly. A *tool* touches the outside world and is realized only through MCP. Bash is the historical exception: a kernel-reserved builtin registered by `an-agent-factory`, pending the MCP slice.

**The document is a grant envelope, not a self-description.** A spool answers four questions: how the code runs (`constructor`), the worst it can touch (`effect` ceiling), what must be present (`requires`), how to unwind it (`inverse`), and what configuration it takes (`config`). What system a spool forms at runtime is not declared — it is observed on the tape as mount/unmount acts and registrations. The document subsists in the registry; the mounted instance is its hypostasis.

**Three layers of discipline:**

1. **Before (admission):** static folds over the pinned requires closure — effect ceilings join (a spool yielding a net-egress member has egress reach), reversibility meets AND (one irreversible member makes the mount irreversible), trust floors at the weakest member. Decidable because the closure is flattened and exact-pinned; publish order is topological, so closures are acyclic.
2. **During (lifetime):** spatial presence is the kernel's RAII registration; a dropped guard unregisters.
3. **After (tape + inverse):** the tape is the settlement — a declaration is a budget, the tape is the audit; deviation between the two is what review looks for. `inverse` names the rewind; nothing consumes it yet.

**Claims are not proofs.** YAML may be hand-written, so static checks bound what admission permits but cannot prove behavior matches the declaration. The honest chain: declaration sets the budget → construction refuses faces the constructor cannot wire → rhai ceilings are physically enforced by host-fn wiring, host ceilings rest on review (hence the trust floor) → the tape reconciles claim against act.

**Registry:** append-only, content-addressed. A published body (`blobs/<sha256>` + `index.jsonl`) is never rewritten; a later version is a new body, so an old workspace write can recover the descriptor that made it.

**Dependencies:** `an-agent-core` (`Tool`, facet vocabulary). Never: `memstream` directly, `principal`.

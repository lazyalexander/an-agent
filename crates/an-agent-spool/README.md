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

**Mount identity:** the host forwards the existing YAML `name` as the stable
package symbol and retains the exact YAML bytes with each mount. Instance aliases
and lifetime IDs remain separate. Other contract fields stay in YAML. This adds
no version ranges, dependency resolver, binary attestation, or hot-update system;
the existing registry version fields remain unchanged for compatibility.

**Dependencies:** `an-agent-core` (`Tool`, facet vocabulary). Never: `memstream` directly, `principal`.

## Execution boundary terminology

A spool release carries domain behavior and internal callable **silks**. Its
execution attempt is the isolation and lifecycle unit owned by the host/backend.
This parser and the existing embedded body APIs do not create that boundary: an
in-process `SpoolBeat` is trusted embedding code, not an isolated workload.
See [the implementation audit and migration](../an-agent-host/SPOOL-BOUNDARY.md).
Historical Silk protocol records and current profile/event tokens remain
compatibility identities; they are not executable silk declarations.

## Registry publication and recovery

`Registry` serializes dependency checks, immutable name/version checks and index
commit with an OS advisory lock on the append-only index file. Independently opened
instances and cooperating local processes use the same lock; published reads take
it too, so they do not interpret a writer's partial append. This reuses the standard
library file-lock facility and introduces no lockfile lease or stale-lock deletion.
Registry paths and index inodes must remain operator-owned; this is not protection
against hostile replacement or a distributed/network-filesystem locking contract.

Blob staging uses unique, exclusively created files. Bytes are synced before rename,
the blob directory is synced on Unix, and the index is synced before publish reports
success. A failed append/sync is an uncertain publication, not rollback. A complete
entry may be recovered idempotently; an incomplete tail, conflicting historical
name/version hashes or mismatched blob identity is refused for inspection. The
registry never silently truncates history or chooses the first conflicting entry.
Unindexed blobs or staging debris from a crash are not automatically reclaimed.
Unlike tape repair, registry history has no automatic tail-repair policy.

## Cascade withdrawal

Scope removal first plans the existing newest-child-first order and simulates
wrapper removal for the whole subtree. A pinned parent, pinned descendant or
incompatible sibling wrapper order refuses the plan before any disposable runs.
Internal wrappers scheduled before their target are allowed; this is not a blanket
ban on wrapped subtrees. Successful removal keeps the existing children-first
report and reverse-registration disposal order. `update` uses the same preflight.

Planning lives in `scope/unmount.rs`; live effect withdrawal remains in `ScopeTree`.
The live pin checks remain defensive during execution. User-supplied disposables
are trusted inverses: arbitrary topology mutation, reentrancy or panic during an
inverse is not made transactional and can still produce partial effects. This
change prevents predictable topology failures, not arbitrary callback rollback.

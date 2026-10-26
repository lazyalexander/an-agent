# memstream

The tape: append-only JSONL event log, the system's single source of truth. Hot path is `context clips`; observation/replay data is cold — indexing, embeddings and graph views are projections built elsewhere, never the source.

**Admission rule:** the event model, storage, and validation. No projections, no query engines.

**Dependencies:** `det_seam`; currently also `act` (envelope and effect types) — see below.

**Invariants:** append-only, never rewritten (one exception: crash-torn tail quarantine — see Durability); readers tolerate schema drift (serde defaults + `skip_serializing_if`); `validate()` on write; new semantics = new fields or kinds.

**Durability:** one write lock spans seq reservation, a single `write`, and `fsync`; `seq` advances only after the durable write. Corrupt bytes are never served as events: a crash-torn final line is quarantined to `<tape>.corrupt` (evidence kept) and truncated — the only permitted rewrite; corruption anywhere else fails the whole store.

**Known structural debt:** `act` and `memstream` import each other (fine as modules, blocks splitting them into crates). Fix direction: sink the envelope types below the tape, or make `ActOnEvent` carry a generic payload. A future slice.

Writer ownership ends with an explicit unlock on store drop. Closing the file
alone is insufficient when a concurrently forked child still holds the inherited
open file description before exec. Inherited descriptors are not independent
writer grants; a new store must acquire its own lock. The host regression holds
such a child alive while the parent drops and immediately reacquires the tape.

A store is bound to the PID that opened it. After `fork`, inherited store methods
reject with `InheritedProcess` before acquiring a mutex, reserving a sequence,
reading or repairing the tape, or accessing a tool payload. Child destruction
closes its inherited descriptor without unlocking the parent's open file
description. Parent destruction still explicitly unlocks, so a fresh writer can
acquire ownership even while an inert inherited descriptor remains open. The
child must open a new store and acquire its own writer lock before using the tape;
inherited objects never become valid merely because the original owner exited.

Regression coverage includes child-side destruction while the parent retains
ownership, stale inherited reads/appends/payload calls after a new writer takes
over, refusal to repair a torn tail owned by that writer, and owner checks before
a locked state mutex. PID checking is an object-lifecycle rule, not a sandbox or
a guarantee for arbitrary application code after a multithreaded fork. The
standalone read-only payload resolver does not own a tape or perform repairs.

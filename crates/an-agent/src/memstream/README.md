# memstream

The tape: append-only JSONL event log, the system's single source of truth. Hot path is `context clips`; observation/replay data is cold — indexing, embeddings and graph views are projections built elsewhere, never the source.

**Admission rule:** the event model, storage, and validation. No projections, no query engines.

**Dependencies:** `det_seam`; currently also `act` (envelope/sentence types) — see below.

**Invariants:** append-only, never rewritten; readers tolerate schema drift (serde defaults + `skip_serializing_if`); `validate()` on write; new semantics = new fields or kinds.

**Known structural debt:** `act` and `memstream` import each other (fine as modules, blocks splitting them into crates). Fix direction: sink the envelope/sentence types below the tape, or make `ActOnEvent` carry a generic payload. A future slice.

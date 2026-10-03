# recorder

Steward of the file workspace. Not the host face, and not the process.

**Admission rule:** only the recorder spawns workers onto this tree. Its world-facing grants are Deny. Spawn, mail, and link here are verbs, not tools. A view is a named list of link ids already on its tape. Each worker has one sub-workspace. Closing it publishes a view, or asks when two true names claim one path. The recorder does not write file bytes. `recover` rebuilds projections from the tape.

**Dependencies:** `agent`, `seat`, `memstream`, `act`. Never: `control`, the workspace CAS store.

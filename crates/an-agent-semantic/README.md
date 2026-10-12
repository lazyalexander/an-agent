# an-agent-semantic

The vertical semantic plane: transport-neutral claims, concrete attempts,
version-bound decisions, grants, and execution receipts. It contains no tools, tapes, workspaces,
spools, host handles, or interaction loops.

**Admission rule:** every authority is an independent claim. A concrete attempt
is allowed only when every supplied claim covers the same subject, operation,
resource, and polarity. No claims means deny. Adapters supply all required
authorities. Grants bind execution id, subject, capability, fact revisions,
and input refs exactly; `Grant::matches` checks that binding, not authenticity
or single-use consumption. Receipts record succeeded, failed, or unknown
execution with the grant and evidence; unknown does not permit a safe retry.

**Boundary rule:** business crates translate domain actions into semantic
attempts. This crate never imports their vocabulary. Trusted adapters enforce
the decision and own the real resource handles; observation alone is not
enforcement.

**Current adapters:** core records allow and deny delivery decisions and model
completion decisions. Model attempts bind the card, mounted declaration,
delivery registration, workspace ingredients, and retained application
request. Terminal model observations carry receipts. Static analysis,
external provenance, idempotency, replay execution, and OS sandbox enforcement
remain later slices.

**Dependencies:** `serde` only. Product crates may depend on this crate; this
crate never depends on them. Policy languages, storage, transport, interaction,
and observation exporters remain replaceable adapters.

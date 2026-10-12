# an-agent-semantic

The vertical semantic plane: transport-neutral claims, concrete attempts,
version-bound decisions, and grants. It contains no tools, tapes, workspaces,
spools, host handles, or interaction loops.

**Admission rule:** every authority is an independent claim. A concrete attempt
is allowed only when every supplied claim covers the same subject, operation,
resource, and polarity. No claims means deny. A grant copies the exact fact
revisions used by the decision; adapters must not widen or reuse it for a later
epoch.

**Boundary rule:** business crates translate domain actions into semantic
attempts. This crate never imports their vocabulary. Trusted adapters enforce
the decision and own the real resource handles; observation alone is not
enforcement.

**Dependencies:** `serde` only. Product crates may depend on this crate; this
crate never depends on them. Policy languages, storage, transport, interaction,
and observation exporters remain replaceable adapters.

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
attempts. This crate never imports their vocabulary or schedules, cancels,
unmounts, or contacts an agent. Its decision is an assessment, not an interrupt
command. Trusted execution boundaries own the handles and enforce hard ceilings;
the control layer stops blocked operations and sends review requests to a
host-bound supervisor. Observation alone is not enforcement.

**Declaration checker:** `assess_contract` accepts an attempt, a boundary profile,
and role-labelled claims tied to exact input facts. It returns allow, deny, or
unknown with findings and explicit coverage limits, never a grant. Missing roles,
duplicate roles, absent/unbound evidence, and uncovered operations cannot produce
allow. Known violations remain deny even when other evidence is incomplete.
Profiles and claim roles come from trusted adapters, not self-asserted YAML.
The same pure function supports offline preflight and runtime comparison without
a tape or host. It checks declarations, not arbitrary program behavior.

**Current adapters:** core records allow and deny delivery decisions and model
completion decisions. Model attempts bind the card, mounted declaration,
delivery registration, workspace ingredients, and retained application
request. A denied delivery or model call returns a review request without
unmounting the body. Terminal model observations carry receipts. The new
contract assessment is recorded alongside the existing decision in shadow mode;
agreement, difference, and incomplete inputs are explicit. Existing admission
still decides execution. Card-configured endpoints are not independent egress
grants, and a delivery profile does not cover a native body's hidden IO.
Language-level static analysis,
external provenance, idempotency, replay execution, and OS sandbox enforcement
remain later slices.

**Dependencies:** `serde` only. Product crates may depend on this crate; this
crate never depends on them. Policy languages, storage, transport, interaction,
and observation exporters remain replaceable adapters.

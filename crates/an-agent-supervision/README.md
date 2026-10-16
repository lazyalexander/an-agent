# an-agent-supervision

Transport-neutral, process-local review state. Depends on `an-agent-semantic`,
not on core, tapes, tools, spool loaders, or host interaction loops.

Tracks live instances, supervisor relationships, mount identities, authority
epochs, pending reviews, and exact one-shot approval bindings. It performs no
IO, starts no work, and does not expand a resource ceiling.

Dependency-change requests cite quarantined boundary evidence. They can be
consulted, rejected, or resolved with a request for fresh work, but never with
ApproveOnce. Resolution does not promote an old result or automatically retry.

The trusted caller authenticates principals, supplies fresh instance IDs,
serializes transitions, verifies hard permissions, and persists evidence.
Approval evidence must succeed before activation; revocation and consumption
remove authority before fallible evidence delivery. A serialized request is
historical evidence, not an authority token.

Standalone tests exercise the state machine without a tape or core runtime.
Core integration tests exercise the real boundary adapter. Existing public
review types remain available through `an_agent_core::control` re-exports.

This ledger is trusted in-memory state, not a durable authorization service.
Closed request IDs are retained for its lifetime. Restart requires fresh
bindings and reviews; there is no automatic authority recovery from history.

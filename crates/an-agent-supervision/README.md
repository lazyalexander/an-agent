# an-agent-supervision

Transport-neutral, process-local review state. Depends on `an-agent-semantic`,
not on core, tapes, tools, spool loaders, or host interaction loops.

Tracks live instances, supervisor relationships, mount identities, authority
epochs, pending reviews, and exact one-shot approval bindings. It performs no
IO, starts no work, and does not expand a resource ceiling.

The trusted caller authenticates principals, supplies fresh instance IDs,
serializes transitions, verifies hard permissions, and persists evidence.
Approval evidence must succeed before activation; revocation and consumption
remove authority before fallible evidence delivery. A serialized request is
historical evidence, not an authority token.

Core integration tests exercise the real boundary adapter. Existing public
review types remain available through `an_agent_core::control` re-exports.

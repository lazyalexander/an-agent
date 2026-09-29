# principal

Agent identity: the `AgentCard` (model spec + prompt + tool grants + kernel ref), an append-only content-addressed registry, and a card-driven factory.

**Admission rule:** identity and construction. No runtime state, no memory, no keys — a card declares memory *access rights*, never contents. Evolution is supersession: new card, `supersedes` edge, old card untouched.

**Dependencies:** `act` (tool traits/tags), `det_seam`. Constructors are passed in; this crate does not register tools. Never: the loop, HTTP.

**Not here:** the live runtime (session, tree, pool), tool implementations, the ReAct loop, and the model client. `runtime::spawn_with` binds a `BuiltAgent` to a private session.

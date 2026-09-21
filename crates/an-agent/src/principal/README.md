# principal

Agent identity: the `AgentCard` (model spec + prompt + tool grants + kernel ref), an append-only content-addressed registry, and a card-driven factory.

**Admission rule:** identity and construction. No runtime state, no memory, no keys — a card declares memory *access rights*, never contents. Evolution is supersession: new card, `supersedes` edge, old card untouched.

**Dependencies:** `act` (tool traits/tags), `tools` (constructors), `det_seam`. Never: the loop, HTTP.

**Not here:** live session/tree/pool (`instance`), the ReAct loop, and the model client. `instance::spawn` is the loop-facing constructor that binds a card to a private session.

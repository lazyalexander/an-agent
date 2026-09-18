# tools

Tool implementations and descriptor admission.

**Admission rule:** tool implementations (today: `bash`, the grandfathered builtin pending rhai migration) plus `descriptor.rs` — strict YAML admission for tool descriptors: parse-or-reject, unknown fields fatal, all four effect faces explicit.

**Constructor focus:** rhai and MCP (contract: tool-registry.md).

**Dependencies:** `act` (the `Tool` trait and facet vocabulary). Never: `memstream` directly, `principal`.

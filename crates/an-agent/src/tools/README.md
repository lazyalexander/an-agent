# tools

Tool implementations and descriptor admission.

**Admission rule:** tool implementations (today: `bash`, the grandfathered builtin pending rhai migration) plus `descriptor.rs` — strict YAML admission for tool descriptors: parse-or-reject, unknown fields fatal, all four effect faces explicit.

**Constructor focus (contract T8):** rhai and MCP. `bash` migrates to rhai + host `exec` when the YAML registry slice lands.

**Dependencies:** `act` (the `Tool` trait and facet vocabulary). Never: `memstream` directly, `principal`.

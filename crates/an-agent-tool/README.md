# an-agent-tool

External tools. The kernel does not depend on this crate.

**Admission rule:** implementations (today: `bash`) plus strict YAML descriptors: parse-or-reject, unknown fields fatal, all four effect faces explicit.

**Constructor focus:** rhai and MCP. The composition crate is what registers bash.

**Dependencies:** `an-agent-core` (`Tool`, facet vocabulary). Never: `memstream` directly, `principal`.

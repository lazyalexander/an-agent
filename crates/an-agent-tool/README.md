# an-agent-tool

External tools. The kernel does not depend on this crate.

**Admission rule:** implementations (today: `bash`) plus strict YAML descriptors: parse-or-reject, unknown fields fatal, effect faces explicit. A `kind: plugin` document is a hosted capability (`rhai` or `host`). `requires` names other published plugins. `inverse` is `irreversible` or another published plugin version. Its body is stored by sha256 and never rewritten. Probe descriptors without `kind` stay on the older parser.

**Constructor focus:** bash registration lives in `an-agent-factory`. An MCP leaf is not this document.

**Dependencies:** `an-agent-core` (`Tool`, facet vocabulary). Never: `memstream` directly, `principal`.

# agent

English · [简体中文](../../../../doc/zh-CN/crates/an-agent-core/src/agent/README.md)

A live card and the private directory that holds its tape.

**Admission rule:** `spawn_with` binds a `BuiltAgent` to a fresh directory. The kernel registers no tools. This is not the host face and not the file workspace.

**Dependencies:** `principal`, `memstream`, `act`. Never: `recorder`, `control`, the workspace CAS store.

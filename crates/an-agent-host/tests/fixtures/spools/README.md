# Host spool fixtures

English · [简体中文](../../../../../doc/zh-CN/crates/an-agent-host/tests/fixtures/spools/README.md)

These fixed bodies exercise the host loading, registration, mount and dispatch
boundaries in `discord_parley.rs` with the `embedded-rhai` feature. They are test inputs,
not a bundled domain library or a product dependency. `chat_npc` wraps a Rhai
script; `discord` supplies the test channel envelope.

Production domain knowledge and release bodies belong in an-spool. The host
retains generic loading, permission checks, lifecycle and artifact acceptance.

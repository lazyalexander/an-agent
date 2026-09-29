# an-agent-factory

Registers the bash constructor, builds a `BuiltAgent`, and opens a session with that list (`spawn`).

**Admission rule:** the constructor list and the session that carries it. Worker seats, charter checks, and the tape writer stay in the runtime.

**Dependencies:** `an-agent-core` (`build_with`, `spawn_with`). Bash lives here: a kernel-reserved builtin, pending the MCP slice.

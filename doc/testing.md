# Testing and cross-repository verification

This contract guides test placement and selective migration. It does not change
runtime behavior, move existing tests or replace current CI commands. Repository
verification remains defined in [AGENTS.md](../AGENTS.md) and the workflows.

## Ownership and language

| Scope | Owner and preferred implementation |
| --- | --- |
| Internal invariants, parsing, identity checks, private fault windows | Rust tests beside the owning module or in existing integration targets |
| Real CLI composition, process cancellation, filesystem custody and recovery | Host acceptance; prefer Python for new black-box suites using built binaries |
| Authoring, declarations, frozen candidates, packaging and evidence reception | Loom tests; Rust for internal contracts, Python for external CLI composition |
| Loom release loaded and executed by the host | Narrow paired acceptance, using explicit Loom and host revisions |
| Browser or GUI integration | Adapter-owned tests; TS when it fits the existing browser tooling |
| Domain correctness | Spool/example-owned tests, not host or Loom business branches |

Language is a testing choice, not a runtime dependency requirement. Keep mature
Rust tests when moving them offers no measured benefit. Do not expose a new
production API merely to let an external test trigger an internal fault window.
Host stress and failure-window matrices belong in an-agent; Loom should verify
its handoff contract without duplicating that full matrix. Existing coverage
must remain until an equivalent replacement is demonstrated.

## Placement and execution

Keep private Rust tests adjacent to their modules under `cfg(test)` and public
Rust suites in the owning crate's `tests/` tree. Future Python CLI suites should
live under that crate's `tests/cli/`, with a local README listing prerequisites,
binary arguments, commands and supported platforms. This is a placement rule,
not a claim that these suites have already been migrated.

Build product binaries in an explicit preparation step, once per selected
revision, target and build configuration. Pass their paths to external tests;
do not use `cargo run` or rebuild the workspace inside each scenario. A test
whose purpose is to verify a build remains an explicit exception. Cargo's
`test --no-run` followed by `test` normally reuses test binaries; these are not
inherently two full compilations.

For paired acceptance, record both source revisions, binary digests, target OS
and architecture, and relevant profile/contract identities. Historical suites
may pin different host revisions; do not silently reuse one binary across them.
A cache is only an optimization, never evidence of matching inputs.

Each external runner must bound waits, preserve failure output, and clean up
only processes and files it owns. A client timeout does not establish execution
termination. Retain evidence and refuse destructive cleanup when termination is
unknown; do not retry the operation automatically. Exercise real enforcement
before claiming it, and label simulated receipts or faults explicitly.

The first migrated macOS suite and assertion map are documented in
[CLI acceptance](../crates/an-agent-host/tests/cli/README.md). It runs in the
existing required macOS CI job using the compiled host binary.

## Migration acceptance

For each selected suite, document old and new entrypoints and map its assertions
and fault cases. Preserve cancellation, stale-result rejection, version pinning,
termination confirmation and artifact-retention checks where applicable. Keep
required CI coverage and supported-platform checks; unavailable prerequisites
are not a passing acceptance result. Coordinate any entrypoint replacement with
CI and downstream callers in the same reviewed change.

Run both versions for the migration comparison, then remove duplicate coverage
only when equivalence is established. Do not weaken gates, raise deadlines or
hide races to obtain a faster result. Standard Cargo checks remain required;
run affected external suites separately and report skipped real acceptance.

Measure dependency/product compilation, test compilation/linking and execution
separately where tooling permits. Record runner/platform, revisions, toolchain,
cache state and repeated measurements; distinguish cold from warm runs. Python
can remove Rust test compilation but cannot remove product compilation. Do not
claim a speedup from a single combined CI duration or a changed runner.

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


## CI dependency caching

Host CI installs the toolchain selected by the repository's `rust-toolchain.toml`
through rustup, including its declared components. Do not install a separate
moving `stable` version before selecting the pinned version. The ephemeral hosted
runner makes that version its default (including fixtures invoking rustc outside
the repository) and removes other installed toolchains for this job only. The
cache action fingerprints all installed compilers; otherwise unused preinstalled
stable-version differences can invalidate a cache for identical source. This
workflow does not change developer machines.

The pinned [rust-cache action](https://github.com/Swatinem/rust-cache) covers both
`. -> target` and `tools/archcheck -> target` in one cache operation. These remain
independent Cargo workspaces and build directories. The action keys dependencies
by compiler and Cargo configuration/lockfiles; our additional key separates
runner OS and architecture. The `host-deps-v1` prefix provides an explicit cache
namespace. Default dependency-only caching keeps project compilation and all
checks active; cached binaries are never treated as passed acceptance evidence.
The action disables Cargo incremental compilation for this CI cache strategy.

Registry/git dependencies and dependency build outputs are cached, not lifecycle
receipts, workspace products, credentials or test results. Installed Cargo
binaries are excluded. Cache restoration cannot skip formatting, compilation,
workspace tests, Clippy, architecture-tool tests/lint or the architecture gate.
Existing job names, OS matrix, timeout and evidence upload remain unchanged.
GitHub's normal cache scope applies; a PR cache is not a new runtime trust grant.

To measure a cache change, first run the PR in a fresh namespace and confirm a
miss in its cache logs. After that run finishes successfully and saves the cache,
rerun the same commit and record the exact-hit observation. Compare per-OS job
and step durations including cache restore/save, not only compilation. A completed
CI rerun is a fresh test run, not a retry of an unknown product operation. Do not
flush unrelated caches or increase timeouts to improve the comparison. Preserve
run/attempt links in the PR and distinguish runner variance from established
improvements. Cache misses remain fully supported and must pass the same gates.

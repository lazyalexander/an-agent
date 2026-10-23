# Repository architecture gates

Run `cargo arch` before opening a PR. It compares against `origin/dev-advance`;
use `cargo arch --base COMMIT` for an explicit, fetched target commit. It never
rewrites the baseline. CI runs the same command inside both existing native-host
checks, using the PR's base SHA (or the previous push SHA), not a moving branch.

## Ownership

- `ARCHITECTURE.toml`: SPACE-compatible size, command-mention and naming rules.
- `architecture-baseline.toml`: existing exceptions and exact caps.
- `architecture-policy.toml`: local advisory thresholds and dependency edges.
- `tools/archcheck`: thin fail-closed wrapper; an independent Cargo workspace
  and lockfile, with no product dependency or runtime coupling.

The SPACE checker is a git dependency pinned to commit
`57c7c0c47a73373a974c5b542d180599795465da`, package
`pplx-space-archcheck`, upstream Apache-2.0, copyright Perplexity AI.
Source and upstream license remain in that dependency; no checker source is
copied into this repository. The product lockfile is unchanged. Initial tool
setup needs access to GitHub and crates.io; later runs use the locked cache.
See [upstream](https://github.com/perplexityai/SPACE/tree/57c7c0c47a73373a974c5b542d180599795465da/crates/tools/archcheck).

## Measured rules, not aesthetic scores

Production files have a 500-effective-line ceiling and a 350-line advisory.
The upstream metric excludes blank lines and leading `//` comments; block
comments, attributes and inline test modules count. Separate test files are
excluded by upstream inventory; the wrapper reports 600-line warnings and an
800-line target **advisory**, not a test-deletion incentive. This initial slice
does not enforce function length or parse Rust for type-resolved process calls.

Command construction mentions are restricted to explicitly named host ownership
boundaries. Existing factory/file-job occurrences are capped, not declared safe.
Platform adapters and the process owner remain legitimate command builders.
The regex is a review lint: aliases, macros or indirect calls can evade it;
strings and inline tests can count. It does not enforce sandbox permissions or
prove that a command goes through the supervisor. Runtime controls and review
remain necessary. We do not impose one global quota on all CLI calls.

Dependency checks use Cargo's declared non-dev dependencies, including target
and optional declarations, following paths across workspace packages. They
check actual package names rather than aliases. External dependency internals
are not expanded. Runtime workspace packages must not depend on Waver or this
checker; core must not depend on host or the selected HTTP client. No blanket
ban on all Tokio features or all filesystem operations is inferred.

## Ratchet and policy changes

Existing oversized files may remain, but cannot grow beyond their listed caps.
New oversized files, new command sites outside reviewed boundaries, raised caps,
new exceptions and stale entries fail. Lower or remove entries when debt shrinks.
Git rename evidence is passed to the upstream engine; moving a file is not a
fresh exception. No automatic baseline reset is provided.

Missing or malformed current/base configuration, unknown configuration keys,
unavailable Git bases, metadata failures and skipped comparisons fail closed.
Only the exact last pre-policy commit
`f54cfa71e463a7b2902a16a26494dc44fce2485a` can bootstrap the initial reviewed
baseline. This anchor is checked in code as well as TOML. It is not a generic
"missing baseline means pass" path.

The first stable-line promotion has one separately reviewed migration: when the
base is exactly pre-gate main `a3ff193c7d9732f2c272125e9b9c925bc9096567`, compare
against rules and baseline committed in the original gate implementation
`8c89829b9de7506925a5d1d107d04df500ddd54f`. That policy commit must exist in HEAD's
ancestry. The gate still checks the current tree, policy equality, new entries,
raised caps and stale entries; rename detection still uses the actual PR base.
It does not copy the current baseline or disable any finding. No branch name,
other older commit, missing file or malformed policy activates this migration.
Once main carries the gate, ordinary base-to-head comparison applies again.

Semantic changes to either rules file fail comparison with the target commit.
An intentional policy migration therefore needs its own explicitly reviewed
change to the gate, not an ordinary feature PR's new exemption. Comments may
change without changing policy. The checker, lockfile, Cargo alias, CI and policy
files are review-sensitive; CODEOWNERS requests owner review. CODEOWNERS alone
does not enable server-side required reviews. Existing required native-host
checks now include the gate; no branch-protection bypass is introduced.

## Refactoring discipline

Split by authority, state ownership and responsibility, not arbitrary line
boundaries. Keep cancellation/acceptance decisions readable as one state machine.
Do not widen public APIs or introduce catch-all helper crates to satisfy a cap.
Preserve comments and tests; a structural move and a behavior change belong in
different commits. Every split still runs workspace tests, clippy and formatting.

Tool checks:

```sh
cargo test --manifest-path tools/archcheck/Cargo.toml --locked
cargo clippy --manifest-path tools/archcheck/Cargo.toml --all-targets --locked -- -D warnings
cargo fmt --manifest-path tools/archcheck/Cargo.toml -- --check
cargo arch
```

Tool tests deliberately corrupt baselines, add forbidden command mentions, grow
files, raise caps and weaken policy. Passing the current tree alone is not
enough evidence that the gate works.

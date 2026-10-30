# Host CLI acceptance

These standard-library Python tests exercise the real, prebuilt macOS host.
They do not build inside scenarios or require Loom, a model, Node or TypeScript.
Linux retains the existing Rust checks; this suite requires macOS enforcement
and fails explicitly when invoked on another platform.

```sh
cargo build -p an-agent-host --bin an-agent-host --locked
AN_AGENT_HOST_BIN="$PWD/target/debug/an-agent-host" \
  AN_AGENT_ACCEPTANCE_DIR="$(mktemp -d)" \
  python3 -B -m unittest discover -s crates/an-agent-host/tests/cli -v
```

CI's workspace test compilation already builds the host binary. The Python step
runs in the existing required macOS job; its evidence uses the existing upload
step. `cargo test` alone no longer runs these two scenarios.

## Migration map

The source was `tests/workspace_job/large_handoff.rs` at revision
`87131cd` (before this migration). Both test names are retained with Python's
`test_` prefix. The remaining workspace-job Rust suites are unchanged.

| Original assertion | Python coverage |
| --- | --- |
| Two 8 MiB zero-filled binary outputs succeed | Same input, script, deadline and status; also checks termination |
| Timing cannot bypass a competing lock | Refusal, conflict trace, no artifact catalog |
| Timing disabled is quiet | No timing marker on preview |
| Commit releases lock and reports copied bytes | `lock_hold/released` and 8 MiB `write_sync` traces |
| Reclaim removes work, accepted versions survive | Work absent; checkout every accepted revision, exact length and bytes |
| Cancel while shell forks 16 sleepers, repeated 32 times | Same script, iterations, readiness polling and five-second bound |
| Cancelled group is confirmed empty | Cancelled receipt, termination confirmed and empty group |
| Cancelled outputs cannot be accepted; work can be reclaimed | Refused accept, approved reclaim; also checks work absent |

Cancellation now enters through SIGINT on the host CLI instead of an in-process
AtomicBool. The CLI prints the receipt and returns 1 for a cancelled execution;
the test checks both, never equating client exit with group termination.
The existing Rust `running_uncertain_and_stale_attempts_cannot_collect_or_resume`
continues to exercise running/unknown termination and stale-attempt refusals.

Commands have bounded waits and retain stdout/stderr and receipts. On cancellation
watchdog failure the runner requests SIGTERM, then bounds client cleanup; it does
not reclaim the workspace or claim detached children stopped. No operation is
automatically retried. Evidence remains in the selected directory (or a unique
system temporary directory); successful large binary copies are removed only
after receipt and custody checks. Operators may retain failed directories for
investigation and remove them only after resolving execution state.

## Local measurement

See [the measurement record](handoff-timing.json). Three trials per implementation
used the same macOS machine, debug profile and warm dependency cache. Build trials
touched the Rust test entrypoint and timed `cargo test -p an-agent-host --test
workspace_job --no-run --locked`; this measures incremental test compilation and
linking, not cold dependency or whole-workspace compilation. Execution trials used
the filtered Rust suite with one test thread, and Python unittest with a prebuilt
host. Rust execution timings include Cargo startup/check overhead. Product code
was unchanged. No cold-build or GitHub runner speedup is claimed.

The incremental compile median changed from about 0.50 to 0.50 seconds; execution
changed from about 4.36 to 5.21 seconds. This small migration does not establish a
speed improvement: additional real CLI launches cost time, and the Rust target
still contains other tests. Its benefit is independent CLI acceptance and a clear
language boundary. Do not extrapolate this result into a broad rewrite.

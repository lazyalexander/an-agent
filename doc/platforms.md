# Platform support and qualification

This is the runtime support matrix for an-agent. Loom owns the corresponding
[authoring and package handoff matrix](https://github.com/lazyalexander/an-loom/blob/dev-advance/docs/platforms.md).
Keep these ownership boundaries aligned; do not copy a runtime claim into every
feature document. A build, a test skip or a manifest declaration is not evidence
that an execution boundary works on that machine.

The current delivery target is local, reviewed **headless** spools. Existing
Linux implementations and CI remain intact; extending Linux execution, GPU/GUI
support or adding a backend is separate work. There is no implicit OS conversion.

OpenShell is retained as an optional backend; expansion is paused. Its existing
adapter and failure tests remain supported within their pinned qualification.
It is not a prerequisite for the native spool lifecycle. Headless describes an
interaction mode, not trust: a workload needing stronger isolation must be
refused until an adequate selected backend is qualified. Never automatically
downgrade to native execution or weaken a declaration to fit it.

## Runtime matrix

| Surface | macOS | Linux | Other platforms | Evidence and limits |
| --- | --- | --- | --- | --- |
| Core, package/record validation and host logic | Workspace CI | Workspace CI | Unqualified | Unit/contract checks are not an enforcement claim |
| Registered stdio and fixed-file profiles | Existing native enforcement | Existing Landlock/seccomp enforcement | Refused/unqualified | `execution` and `file_runtime` tests; [Linux prerequisites](qualification.md); missing enforcement refuses |
| Reviewed workspace execution and native shell/Python spool lifecycle | Real headless CLI acceptance | Execution remains unsupported | Execution remains unsupported | [Host CI](../.github/workflows/host.yml), [CLI cases](../crates/an-agent-host/tests/cli/README.md); reviewed tools, no arbitrary hostile-agent claim |
| Independent supervisor for one native file, checkpoint or combined data/state attempt | Real CLI-loss, timeout and cancellation acceptance | Execution remains unsupported | Execution remains unsupported | Explicit supervised entry; event/session/peer migration separately qualified |
| Isolated Rust build recipe | Existing narrow build/file acceptance | No Linux build adapter | Unqualified | [Build/file contract](workspace-jobs.md); separate from running an already-built file program |
| Optional OpenShell adapter | Operator-selected control/transport | Selected backend environment, independently qualified | Unqualified | [Historical live evidence](openshell.md); opt-in real-VM test is not implied by ordinary CI |
| GPU workers, GUI shuttles, live memory suspension | Separate qualification required | Separate qualification required | Separate qualification required | Neither CPU success nor a native binary establishes these capabilities |

CI runner architecture, selected binary/toolchain and runtime dependencies are
part of the evidence. Do not generalize one runner's success to all CPUs or
kernel versions. Protocol inspection and legacy instance loading may remain
possible where execution is unsupported; retain those existing distinctions.

## Implementation boundaries

- Native policy, process ownership and platform selection live under
  `crates/an-agent-host/src/execution/sandbox/`. The reviewed workspace adapter
  owns its qualification predicate and existing runtime/profile identifiers.
  Spool lifecycle, requirements, release review and workspace planning consult
  that adapter instead of independently selecting an OS.
- Dedicated filesystem adapters, such as APFS snapshot cloning and executable
  publication, keep their own syscall/compile-time boundaries. This is not a
  reason to move every `cfg` into a global platform framework or weaken file safety.
- Domain code, event routing, state selection and artifact custody remain platform
  independent. They still require actual grants and original settlement evidence.
  A qualification flag is eligibility, not proof of installed enforcement.
- `macos-native-process-v1`, `macos-native-python-v1` and existing backend/profile
  IDs are compatibility identities. They remain in manifests, receipts, fixtures
  and adapter validation; this cleanup does not rename them or change approval
  hashes, historical bytes, release digests or Linux refusal behavior.

There is no new capability registry, environment probe service, dependency
resolver or runtime negotiation protocol in this change. Future support must be
implemented and qualified at the relevant adapter, then reflected in this matrix.

## Acceptance entry points

Python native CLI suites use their local `host_qualification.py` entry helper.
It keeps the existing native-workspace prerequisite in one place, before creating
fixtures or invoking a host. It does not execute a probe, fall back to another
backend, or grant execution. Existing `native_state.py` unsupported-platform
unittest skips stay skips; the other explicit runners refuse unsupported starts.
Neither counts as successful runtime qualification.

The host CI matrix selects the native-workspace lane once while preserving the
two required job names, all original commands, assertions, deadlines and evidence
uploads. Native and Linux Rust checks still run in their original lanes. Tests
that directly exercise OS syscalls retain compile-time test boundaries.

Loom independently owns its test-entry prerequisite and pins an exact host
revision for real package/handoff tests. It does not import host implementation
code or infer authorization from the test helper. When changing support, update
the owning adapter, this matrix, the paired Loom handoff reference and relevant
real acceptance together. Preserve historical reports as reports, not current
support tables. No unsupported lane may be reclassified as success to pass CI.

# Host execution and embedding

A spool is the domain execution isolation and lifecycle boundary. Internal
**silks** are callable computations sharing its granted environment; they do not
each receive a VM. See [the boundary, implementation audit and migration](SPOOL-BOUNDARY.md).
OpenShell is optional and replaceable. Existing `kit` profile/CLI names below
remain compatibility spellings; multi-silk execution is not implemented yet.

This crate owns host-side effects, not kit business logic. The library's
`execution::{check, register, run}` and `an-agent-host` CLI implement a narrow
stdio capability and a separate [bounded build/file profile](FILE-JOBS.md).
Fixed-file execution supports macOS and native Linux. The isolated Rust build
recipe remains macOS-only; see [Linux file permissions](LINUX-VALIDATION.md#fixed-file-runtime).
Core remains unchanged. `Host::pump` keeps its existing event
semantics; a fallible process is not executed inside its infallible text sink.

The optional [OpenShell VM adapter](OPENSHELL.md) runs the same file-kit contract
in an independently owned VM. It gates artifact publication on successful
execution and confirmed remote stop, with explicit uncertain termination results.
The [local lifecycle assessment](OPENSHELL-RECOVERY.md) distinguishes backend
cold workspace restart from VM pause, memory suspension and whole-VM backups.
Its opt-in test uses the real host first, then explicit operator inspection;
the production adapter still never restarts a settled run.

Native and OpenShell now implement one internal host-owned execution backend
contract. The host retains admission, operation/result reconciliation and
artifact acceptance; each backend retains enforcement and termination evidence.
See [the execution boundary](ARCHITECTURE.md#internal-execution-backend-contract).
Existing CLI commands, profiles and receipt fields remain compatible; operation
intent and settlement observations are additive, not a new VM control plane.

## Responsibility and dependency direction

A spool carries domain knowledge: business rules, skills, workflows and domain
acceptance criteria. The host manages execution and artifact identities without
learning those rules. [Artifact revisions](ARTIFACT-VERSIONS.md) provide a bounded
accepted-output -> editable copy -> new checkpoint -> pinned downstream input
loop. A VM working disk is not the authoritative artifact catalog.

[Settled work reclamation and mediated capabilities](RETENTION-CAPABILITIES.md)
add explicit native work-copy cleanup and an approved `artifact.read` handoff.
Unconfirmed termination cannot authorize cleanup; accepted products and audit
remain intact. Kits request access but never select host paths or self-grant it.

[Instance state commits](INSTANCE-STATE.md) separate immutable releases,
long-lived instances and fresh execution attempts. Approved commits publish
state, product and report together and reject stale state versions; domain
transitions remain in kits.

[Checkpoint manifests and delivery evidence](CHECKPOINT-DELIVERY.md) add an
opt-in product handoff to fixed recipients. Consumption commits with downstream
state; missing acknowledgements can be reconstructed without running the kit
again. Retained state references block product reclamation even after delivery.

The [architecture contract](ARCHITECTURE.md) separates workflow packages, runs,
stages, CLI kits, application-side shuttles and trusted host/backend authority.
The [local VM direction](LOCAL-VM.md) separates host authorization and artifact
ownership from backend enforcement and VM state. Local-first execution does not
require a remote orchestration platform. Suspend/resume, VM pause and backups
remain unimplemented capabilities, not promises made by the current adapter.
The unused `space-preflight` CLI and library module have been retired; existing
native execution and OpenShell profiles are unchanged.

See [the architecture boundary](ARCHITECTURE.md) for the separation of explicit
dependency updates, optional attention policy and execution authority. Attention
management remains a design boundary; no GNN or new core subsystem is introduced.

```text
developer / code tool -> Loom -> deployment files
                                      |
                                      v
                       an-agent-host -> core + spool
                                      |
                                      v
                              OS sandbox -> kit
```

The runtime never imports Loom, parses its build archive, trusts its source
analysis, or runs package installation hooks. Loom owns scaffolds, source
evidence, packaging and code-tool briefs. Kit authors own algorithms, domain
validation and their native toolchain. The host owns execution policy, bounded
context transfer, audit, and artifact files. Existing model clients and context
assembly remain separate services: this profile grants no model access.

## Deployment contract: stdio-only-v1

A deployment directory supplies `spool.yaml` and `program`. Only those files
are read; other development files are not installed. The existing spool parser
is authoritative. The host additionally requires:

- `sort: beat`, `constructor: host`, `host: an-agent-stdio-v1`;
- `consumes: [kit.request]`, `produces: [kit.execute]`, no dependencies;
- file none, memory ignore, network none, process spawn, model none, flow both;
- `inverse: irreversible` (no replay or rollback promise);
- config `profile: stdio-only-v1` and `executable_sha256`; optional
  `source_sha256` is producer evidence only, never a safety or compiler proof.

Unknown configuration is rejected. The host uses a fixed entry point with no
arguments, shell, environment forwarding or package-defined policy. Process
spawn describes the host's single launch, not permission for child processes.
System runtime reads are backend necessities, not arbitrary workspace access.

The deployment ID is SHA-256 of the exact YAML bytes, transitively binding the
program hash. It is deliberately **not** a Loom archive ID. YAML is bounded to
64 KiB and the executable to 20 MiB. Changed bytes require a new registered
version. Identical registration is idempotent; execution is not deduplicated.

```sh
cargo install --path crates/an-agent-host --locked
an-agent-host check deployment
an-agent-host register deployment host-store
an-agent-host run host-store DEPLOYMENT_DIGEST input.txt
an-agent-host run host-store DEPLOYMENT_DIGEST input.txt --execute
```

The default is preview. Explicit execution prints the resolved plan first and
authorizes only this bounded profile. `--execute` is an operator interface, not
an authenticated remote authorization protocol. Do not give an untrusted model
an unrestricted shell and call that supervised execution. External code tools
must retain their own command approval policy.

## Actual enforcement and evidence

See [native Linux validation](../../infra/validation/native-linux.md) for an isolated
developer VM recipe and explicit stdio output/context handoff. Fixed-file runtime
acceptance is recorded separately in [Linux validation](LINUX-VALIDATION.md).
Neither qualifies a production VM backend.

See [model interruption](MODEL-CLI.md#cancellation-and-deadlines) for approved
HTTP deadlines, cancellation, retained context, and no-retry settlement.
See [process lifecycle ownership](PROCESS-LIFECYCLE.md) for cancellation,
deadline handling, scoped pipe cleanup, confirmed reaping and crash limitations.
See [CLI restart diagnosis](../../infra/validation/cli-restart.md) for the
product executable publisher, concurrent-host regression and required CI gates.
Registration, run snapshots and macOS build acceptance use fixed `/bin/cp` in
a supervised trusted process to create the executable inode, verify its bytes,
and publish without replacing existing files. The embedding process never holds
a writer on that inode. There is no shell, project-selected helper or retry.
The run preview declares the helper's separate ten-second deadline; receipts
separate publication evidence from kit process evidence. This does not replace
the kit sandbox. macOS is the current primary platform; existing Linux CI is
retained without expanding deployment support.

Refusal receipts include
additive `error` evidence (stage, optional OS code, I/O kind and no-retry flag);
the standard process API cannot distinguish spawn from pre-exec policy failure.

For the stdio profile, macOS Seatbelt denies writes, arbitrary file reads, networking and child process
creation. Only the selected executable, OS runtime libraries, random devices,
and necessary system metadata are accessible. The literal root directory is
readable for dyld; this can expose root entry names, not subtree file contents.
An empty environment, 8 KiB UTF-8 input, 64 KiB per output stream and five-second
wall deadline bound one run. Output is drained concurrently with input delivery.
Linux uses native Landlock filesystem restrictions, a seccomp syscall allowlist,
and resource limits. It requires a non-root host and Landlock ABI 3 or newer;
missing enforcement fails closed. No Docker daemon or image is required.
Unsupported systems refuse execution, retaining a refusal receipt. There is no
unrestricted fallback. Nested sandbox activation failure is a failed test, not
a reason to disable the sandbox.

The native shuttle only maps a request event to an intent. Actual core
registration, described mounting, delivery, intent admission and tape recording
precede the subprocess. `applied` is recorded only after successful execution
and artifact/receipt writes. A nonzero exit or backend refusal never gets a
successful applied record. Preflight rejections can occur before a run exists;
admission halts retain the core audit without starting a process.

```text
host-store/
  registry/                         descriptor registry
  releases/<deployment-digest>/     spool.yaml + program
  runs/<run-id>/
    plan.json                       exact invocation, bounds, input hash
    input.txt                       explicit context snapshot
    operation.json                   host operation identity before submission
    program                         selected executable
    stdout.txt, stderr.txt          bounded results
    receipt.json                    identities, outcome, evidence paths
    agent/                          workspace events and thread tape
    audit.json                      readable tape snapshot
```

Only the selected input bytes enter the kit; they are labelled caller-supplied
data, not trusted instructions or verified facts. Model histories, environment
secrets, other spool state and the host store are not exposed. Results are data,
not authority. Native context isolation is tested with an outside-file probe.

## Limits, deliberately explicit

This is an OS-like host responsibility boundary, **not a container manager**.
Linux has per-process address-space and CPU limits; macOS does not yet have
equivalent quotas. There are no arbitrary native dependency mounts, multi-user
service protection, syscall-denial event telemetry, or guaranteed crash recovery.
The store must be private and trusted; static symlink checks are not protection
from hostile same-user concurrent renames. A lock serializes shared metadata operations, not independent child lifetimes;
after a crash, investigate before manually removing a stale lock.

Retention is manual. Failed and partial runs remain inspectable. Do not delete
evidence by age or overwrite external documents. Retaining input and executable
bytes supports investigation, not exact re-execution or recovery of the external
world. The existing content-addressed workspace crate is not duplicated or
silently wired into the recorder here. Broader versioning, reference-aware
cleanup and container backends need their own concrete consumer and contract.

## Internal module boundaries

The [reviewed workspace job profile](WORKSPACE-JOBS.md) adds explicitly approved
local workflow execution and scratch custody on macOS. It reuses the supervisor
and artifact catalog, separates output acceptance from engine recovery data, and
requires an explicit decision to abandon resume before collection. It is a
trusted-tool profile, not arbitrary hostile process-tree containment or a wider
file-kit grant. Domain workflow engines and Loom remain outside the host.

`execution/sandbox.rs` owns one complete process supervision flow, including
bounded streams, deadlines and cancellation. Its regression suite lives in
`execution/sandbox/tests.rs`; platform policy and child ownership remain in the
existing `sandbox/{macos,linux,process}.rs` modules.

`file_job.rs` retains platform policies and process launch sites.
`file_job/build.rs` owns preview/approval, admission and build receipts;
`file_job/artifacts.rs` owns regular-file and output-inode acceptance. Existing
public and crate-internal entry paths remain unchanged. These are source-code
boundaries, not additional runtime stages or authorization services.

## Acceptance

`cargo test --workspace`, `cargo clippy --workspace --all-targets`, and
`cargo fmt --all -- --check` cover the actual host and kernel path. Host tests
construct deployments without Loom, reject wider permissions, mutation,
symlinks, oversized context and version conflicts, and test actual Seatbelt
denials, deadlines and output caps. macOS execution tests must run where the OS
permits Seatbelt activation. Linux tests require native enforcement, never mock success.

## Model and context boundary

The [artifact-context exporter](CONTEXT-OUTPUT.md) turns one receipt-checked kit
or model output into a bounded, provenance-linked context packet without changing
the original artifact or calling a model. It reuses the existing context ABI.
Select `artifact` for a file kit's accepted product, not its diagnostic stdout.

Use the host-owned `ModelConnection` and explicit `an-context-v1` packets for
bounded text exchange. See [the contract](MODEL-CONTEXT.md) and
[security layers and platform boundaries](SECURITY-LAYERS.md). These additions
do not give the stdio-only kit network access or expand core.

The [model CLI](MODEL-CLI.md) adds preview-bound approval for one text request,
retained output and audit evidence. Linux native enforcement is independent of
the optional Docker development environment.

The [finite coding-agent bridge](AGENT-TOOLS.md) exposes selected source, source
proposals and explicitly granted kit handles over the official MCP SDK. It uses
the existing native file executor and artifact custody. It is not a sandbox for
the third-party coding client, and grants no shell or arbitrary filesystem tool.
See [the platform boundaries](PLATFORMS.md) before claiming cross-platform support.

## Release-bound native spool

The bounded [native spool facade](NATIVE-SPOOL.md) loads verified silk/kit releases,
edits scoped kit copies between runs, executes fixed snapshots and retains
instance-bound custody evidence. It reuses the native supervisor and workspace
engine. Pause, suspension and arbitrary harness containment remain unsupported.

The native spool facade also provides an optional [weaver proposal slot](NATIVE-SPOOL.md#optional-weaver-proposal-slot): scoped file requests and separately approved kit changes, without launching a model or granting the adapter host authority.

For a bounded local delivery inventory use `spool-events-pending WORKSPACE`;
`spool-events-watch WORKSPACE SECONDS INTERVAL_MS` emits changed snapshots as
newline-delimited JSON. Monitoring grants no execution and never selects state
or retries work. See [the event contract](NATIVE-EVENTS.md#delivery-inventory-and-foreground-monitoring).

For finite reviewed event batches use `spool-events-dispatch WORKSPACE REQUEST_JSON`:
preview first, then approve its exact hash. SIGTERM drains the selected entry and
stops; SIGINT requests cancellation. State selection is explicit per entry;
acceptance/commit/completion stay separate. Recorded batches never resume on
repeat invocation. See [finite dispatch](NATIVE-EVENTS.md#finite-approved-dispatch).

`spool-event-pause` / `spool-event-resume` preview and approve subscription
admission control. They do not cancel execution. Resume invalidates old delivery
approvals and cannot revive revoked routes or replay batches. First use upgrades
the workspace format so earlier hosts refuse it; see [subscription control](NATIVE-EVENTS.md#subscription-admission-pause-and-resume).

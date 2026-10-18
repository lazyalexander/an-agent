# Host execution and embedding

This crate owns host-side effects, not kit business logic. The library's
`execution::{check, register, run}` and `an-agent-host` CLI implement a narrow
stdio capability. Core remains unchanged. `Host::pump` keeps its existing event
semantics; a fallible process is not executed inside its infallible text sink.

## Responsibility and dependency direction

```text
developer / code tool -> Waver -> deployment files
                                      |
                                      v
                       an-agent-host -> core + spool
                                      |
                                      v
                              OS sandbox -> kit
```

The runtime never imports Waver, parses its build archive, trusts its source
analysis, or runs package installation hooks. Waver owns scaffolds, source
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
program hash. It is deliberately **not** a Waver archive ID. YAML is bounded to
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

macOS Seatbelt denies writes, arbitrary file reads, networking and child process
creation. Only the selected executable, OS runtime libraries, random devices,
and necessary system metadata are accessible. The literal root directory is
readable for dyld; this can expose root entry names, not subtree file contents.
An empty environment, 8 KiB UTF-8 input, 64 KiB per output stream and five-second
wall deadline bound one run. Output is drained concurrently with input delivery.
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
There are no memory/CPU quotas, arbitrary native dependency mounts, multi-user
service protection, syscall-denial event telemetry, or guaranteed crash recovery.
The store must be private and trusted; static symlink checks are not protection
from hostile same-user concurrent renames. A lock serializes cooperating commands;
after a crash, investigate before manually removing a stale lock.

Retention is manual. Failed and partial runs remain inspectable. Do not delete
evidence by age or overwrite external documents. Retaining input and executable
bytes supports investigation, not exact re-execution or recovery of the external
world. The existing content-addressed workspace crate is not duplicated or
silently wired into the recorder here. Broader versioning, reference-aware
cleanup and container backends need their own concrete consumer and contract.

## Acceptance

`cargo test --workspace`, `cargo clippy --workspace --all-targets`, and
`cargo fmt --all -- --check` cover the actual host and kernel path. Host tests
construct deployments without Waver, reject wider permissions, mutation,
symlinks, oversized context and version conflicts, and test actual Seatbelt
denials, deadlines and output caps. macOS execution tests must run where the OS
permits Seatbelt activation. Linux tests require refusal, never mock success.

## Model and context boundary

Use the host-owned `ModelConnection` and explicit `an-context-v1` packets for
bounded text exchange. See [the contract](MODEL-CONTEXT.md) and
[security layers and platform boundaries](SECURITY-LAYERS.md). These additions
do not give the stdio-only kit network access or expand core.

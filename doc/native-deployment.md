# Registered native deployment

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

See [native Linux validation](qualification.md) for an isolated
developer VM recipe and explicit stdio output/context handoff. Fixed-file runtime
acceptance is recorded separately in [Linux validation](qualification.md).
Neither qualifies a production VM backend.

See [model interruption](model-services.md#cancellation-and-deadlines) for approved
HTTP deadlines, cancellation, retained context, and no-retry settlement.
See [process lifecycle ownership](workspace-jobs.md) for cancellation,
deadline handling, scoped pipe cleanup, confirmed reaping and crash limitations.
See [CLI restart diagnosis](qualification.md) for the
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

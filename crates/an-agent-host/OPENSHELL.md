# Optional OpenShell VM adapter

`run-openshell` is a host-side backend for the existing `file-kit-v1`
deployment contract. It does not change core, require Waver at runtime, install
packages, or interpret workflow business logic. Native execution is unchanged.
This first adapter qualifies OpenShell **0.1.5** on macOS with its VM driver
and Linux ARM64 executables. Other platforms fail closed; there is no fallback.

```text
Waver -> deployment (spool.yaml + Linux ARM64 program)
                       |
                       v
an-agent-host: registration / admission / input snapshot / audit
                       |
                 OpenShell adapter
                       |
        operator-owned OpenShell gateway + VM driver
                       |
             dedicated VM -> CLI kit
                       |
      bounded candidate -> confirmed VM stop -> host artifact
```

## Operator setup and invocation

Install the official matching OpenShell CLI, gateway and VM driver, and configure
a dedicated gateway with authenticated access. Gateway installation, credentials,
base-image acquisition and capacity are operator responsibilities, not spool
permissions. This adapter does not download tools or start a gateway. Keep its
configuration, host store and binaries inaccessible to untrusted writers.

Create an operator-owned JSON profile (replace every placeholder):

```json
{
  "cli": "/absolute/path/to/openshell",
  "cli_sha256": "<64-character SHA-256 of the installed CLI>",
  "config_home": "/absolute/path/to/isolated-config",
  "gateway": "kit-validation",
  "image": "nvcr.io/nvidia/base/ubuntu@sha256:<verified image digest>",
  "wall_seconds": 2,
  "control_seconds": 90
}
```

The gateway must report only the VM compute driver. CLI and gateway version are
checked; the CLI hash is rechecked for each operation. This is deployment
qualification, not cryptographic attestation of the gateway. Supply an approved
digest-pinned image with `/bin/sh`, `/bin/sleep`, `/bin/chmod`, `/usr/bin/head`
and the normal system runtime paths. No host directory is mounted.

```sh
an-agent-host register deployment host-store
an-agent-host run-openshell operator.json host-store RELEASE input.txt
an-agent-host run-openshell operator.json host-store RELEASE input.txt --execute
```

Preview does not submit a VM. The host reuses registration, immutable executable
publication, spool admission and audit. `/work` in the existing declaration maps
to guest `/sandbox`; the fixed ABI remains `input.txt` -> `output.txt`.
System runtime reads and guest `/tmp` are explicit backend necessities. There
are no attached providers, forwarded ambient credentials or allowed network
destinations. The host copies only the program and input snapshot into the VM.
Hard Landlock is required. These settings do not prevent a kit from computing
or generating more code inside its authorized guest filesystem.

## Cancellation, deadlines and publication

Each invocation creates a separate, never-restarted sandbox with a random name
and a host run ownership annotation. Shared/reused sandboxes are not supported.
The host records ownership before submission and each control request/result.
No create or execution request is automatically retried.

1. Verify gateway, create VM, bind sandbox ID and runtime generation.
2. Upload program/input and execute with a **local supervisory deadline**.
3. On success only, capture one bounded UTF-8 candidate (at most 64 KiB).
4. On every post-submission outcome, verify ownership, request sandbox stop and
   separately observe `Stopped` for the same identity and runtime generation.
5. Publish only a successful candidate after confirmed stop and a final
   cancellation check. Timeout/cancellation never initiates candidate retrieval.

SIGINT/SIGTERM set the host cancellation flag. Cleanup uses its own bounded
deadline and is not cancelled by that flag. The execution deadline is **not** a
promise that remote compute stops at that instant: control-plane latency remains.
OpenShell CLI exit or its `--timeout` alone is not remote termination evidence.
If stop cannot be confirmed, status is `termination_unconfirmed`, not success or
clean cancellation, and no artifact is published. Stop reason is retained.

The accepted file is a bounded untrusted snapshot, not proof of domain validity
or a quiescent whole-filesystem snapshot. A background process may change guest
bytes during capture; it cannot cause extra files to be imported. A late file
never upgrades an already failed run. Receipt `artifact_accepted` records host
publication; adapter `candidate_eligible` records the earlier lifecycle decision.

## Evidence and recovery limits

Run directories retain input/context hashes, request/result journals,
`openshell-owner.json`, `openshell.json`, `receipt.json` and the existing audit.
Receipt process PID/reaping describes the local CLI only. Remote termination is
reported separately. Candidate contents are not duplicated into control logs.
Diagnostics can contain paths and guest text: treat the run directory as private.

Stopped sandboxes are retained for explicit operator inspection/removal. A host
crash, SIGKILL, disk failure or unavailable gateway can leave an uncertain run.
This version does not provide a durable reconciliation daemon. Use the owner
journal to inspect the named sandbox, verify its run annotation, stop it, and
confirm its state. Do not restart it or adopt its output automatically. No disk
rollback, transactional external-effect reversal or general security proof is
claimed. SPACE integration and provider/model routing are outside this slice.

## Verification

Regular unit tests cover timeout, cancellation after candidate capture, failed
stop, identity replacement, ambiguous submission and oversized output. They
verify adapter decisions, not VM isolation.

The opt-in real test needs an already configured gateway, the existing file-list
kit built for `aarch64-unknown-linux-musl`, and
`tests/fixtures/openshell_probe.rs` compiled for that same target. The probe
creates an early output and a delayed-write descendant. Use absolute canonical
paths and a fresh evidence directory:

```sh
AN_OPENSHELL_PROFILE=/absolute/operator.json \
AN_OPENSHELL_EVIDENCE=/absolute/new-evidence \
AN_OPENSHELL_KIT=/absolute/file-list-linux \
AN_OPENSHELL_PROBE=/absolute/probe-linux \
cargo test -p an-agent-host --test openshell_live -- --ignored --nocapture
```

Use `wall_seconds: 2` so the timeout precedes the probe's eight-second late write.
The test leaves VM state and evidence intact. After more than eight seconds,
an operator can separately start each stopped probe VM, verify `late.txt` is
absent, and stop it again; this diagnostic restart is not adapter behavior.

### Recorded acceptance (2026-10-23)

Apple Silicon host, official OpenShell CLI/gateway/VM driver 0.1.5, ARM64 Ubuntu
image pinned to digest
`7e0f726d635d440d28a7f18e6498d3bdb76c7038bd4c90a090c21eb7c9f40686`:

| Case | Host result | Remote stop confirmed | Artifact |
| --- | --- | --- | --- |
| Existing file-list kit, unchanged Linux binary | succeeded | yes | `alpha\nbeta\n` |
| Hanging kit with eight-second delayed descendant; two-second deadline | timed_out | yes | rejected |
| Same kit, cancellation after execution submission | cancelled | yes | rejected |

The real integration test completed all three cases in 58.09 seconds, including
VM provisioning and teardown. After each delayed-write deadline, separate
operator inspection restarted the two stopped probe VMs: the early output
existed but `late.txt` did not. Both VMs were stopped again after inspection.
This distinguishes a running probe from a cancellation that happened before
launch. The accepted file-list output SHA-256 was
`e49c81e2d2f84e259d40e2fb8192f3bcd198b355184845d76d8f58807d0d78ee`.

An earlier direct OpenShell CLI evaluation (without this adapter) observed a
delayed write after `sandbox exec --timeout` and after interrupting the local
CLI. That observation is why this adapter explicitly stops the VM rather than
equating local process exit with remote cancellation. These tests are bounded
acceptance evidence, not proof against arbitrary VM escapes or control-plane
failure. Raw logs, local paths and TLS material are deliberately not published.

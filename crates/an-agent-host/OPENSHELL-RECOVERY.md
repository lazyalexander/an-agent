# OpenShell local lifecycle assessment

Scope: the already qualified OpenShell CLI/gateway/VM driver **0.1.5** on
Apple Silicon, source revision
`bdb956f2cd65a635f1294fccf1d138d263bf650f`. This is a bounded backend assessment,
not a claim about every OpenShell release or every libkrun capability.

## What can actually be reused

| Requirement | Observed interface/implementation | Decision |
| --- | --- | --- |
| Pause admission | A host scheduling decision, not a compute-driver RPC | Defer until there is a real multi-stage consumer |
| Freeze a running VM | No pause/unpause RPC in the selected compute contract or CLI | Unsupported by this adapter; do not emulate with a launcher signal |
| Suspend and resume memory | No memory checkpoint/restore RPC in the selected contract | Unsupported; stop/start is not equivalent |
| Stop and cold-restart a workspace | Backend stop preserves disk state; start reuses the overlay with new launch authentication | Reuse for an explicit operator acceptance test |
| Whole-VM backup/restore | No export/import backup RPC in the selected contract | Do not copy private driver state and call it a supported backup |
| Retain accepted kit outputs | Existing host receipts, hashes and immutable accepted copies | Keep this boundary; it is not whole-VM recovery |

The CLI's workload templates describe images, resources, environment and driver
configuration. They do not capture a running machine's disk and memory. In
source, many occurrences of “snapshot” mean a resource/status observation rather
than a restorable VM image.

## Source evidence

- [Compute-driver contract](https://github.com/NVIDIA/OpenShell/blob/bdb956f2cd65a635f1294fccf1d138d263bf650f/proto/compute_driver.proto):
  `StopSandbox` and `StartSandbox` are present; pause, suspend and backup RPCs
  are absent. Start carries a generation and fresh launch credentials.
- [VM driver](https://github.com/NVIDIA/OpenShell/blob/bdb956f2cd65a635f1294fccf1d138d263bf650f/crates/openshell-driver-vm/src/driver.rs):
  `stop_sandbox` persists a stop marker, terminates processes and removes runtime
  authentication material. `start_sandbox` validates the persisted request,
  installs new generation/authentication and restores the retained overlay.
- The same driver's `terminate_sandbox_processes_within` allows bounded guest
  shutdown/flush time, then terminates the VM. A forced supervisor termination
  can leave recent writes unflushed even when the VM ultimately stops.
  **Stopped is termination evidence, not application-consistent backup evidence.**
- [VM driver overview](https://github.com/NVIDIA/OpenShell/blob/bdb956f2cd65a635f1294fccf1d138d263bf650f/crates/openshell-driver-vm/README.md):
  immutable bootstrap/root images and per-sandbox writable overlays belong to
  the backend. Do not mount or edit these private disks from the host adapter.
- [Workload templates](https://github.com/NVIDIA/OpenShell/blob/bdb956f2cd65a635f1294fccf1d138d263bf650f/docs/how-it-works/sandboxes/templates.mdx)
  are configuration reuse, not backups.

## Minimal acceptance and responsibility boundary

The opt-in test first invokes the real host registration, admission, controlled
file-kit execution, confirmed stop and accepted-artifact path. Only then does
the operator test directly request backend restarts. Production
`run-openshell` remains one sandbox per run with no restart or automatic retry.
The test reuses the existing CLI transport, bounded process supervisor and
request/result journals; it introduces no new production VM API.

The witness distinguishes disk recovery from execution continuation:

1. Confirm the host's settled sandbox ID, generation and stopped state.
2. Explicitly start it; require the same sandbox and a new runtime generation.
3. Run a process that writes a checkpoint and boot identity, syncs, and would
   write a late marker after twelve seconds. Observe readiness before stopping.
4. Confirm stop and refusal of commands while stopped.
5. Start again, verify retained checkpoint and kit output, a changed boot ID,
   another generation and unchanged effective policy.
6. Wait beyond the old process delay; require that its late marker is absent.
7. Mutate only the guest output during inspection; require that the host's
   accepted artifact and original receipt remain byte-identical.
8. Verify ownership and confirm a final stop, retaining all evidence.

This is a cooperating checkpoint fixture, not database consistency, crash
recovery, hardware migration, credential replay testing or VM escape testing.
Generation rotation is observable; it does not by itself prove that every old
credential was rejected. Existing mocked generation-mismatch tests continue to
protect the host's stop/acceptance boundary.

## Running the opt-in test

Use the existing [operator setup](OPENSHELL.md). Listener TLS and VM launch
signing are separate requirements: configure the gateway's launch signing
material as documented by OpenShell. A healthy listener alone is insufficient.
Do not weaken authentication to make the test pass.
When starting the gateway with a cleared environment, explicitly configure its
XDG config, state and data directories. The VM launcher needs its runtime data
directory even when the gateway itself starts successfully. These are trusted
operator settings, never permissions granted to a kit.

Use canonical absolute paths, a fresh evidence directory, the qualified
digest-pinned image, and the unchanged Linux ARM64 file-list kit:

```sh
AN_OPENSHELL_PROFILE=/absolute/operator.json \
AN_OPENSHELL_EVIDENCE=/absolute/new-recovery-evidence \
AN_OPENSHELL_KIT=/absolute/file-list-linux \
cargo test -p an-agent-host --lib real_workspace_cold_restart -- --ignored --nocapture
```

The image must provide `cat`, `sync`, `sleep` and `/bin/sh` in addition to the
existing adapter requirements. The test keeps the stopped VM and private
evidence; it never deletes unrelated sandboxes or overwrites an earlier run.
Inspect `cold-recovery.json` and the control journals. A failed initial host
run retains the normal host receipt instead. Do not publish configuration,
machine identities, gateway credentials or raw guest/control logs.

## Recorded acceptance (2026-10-23)

Apple Silicon, CLI/gateway/VM driver 0.1.5, existing Linux ARM64 file-list kit,
Ubuntu image digest
`7e0f726d635d440d28a7f18e6498d3bdb76c7038bd4c90a090c21eb7c9f40686`.
The opt-in test passed in **60.93 seconds**. A second fresh run with explicit
stopped-phase refusal and all-generations-distinct assertions passed in
**59.90 seconds**. Both runs finished with confirmed stop.

| Observation | Result |
| --- | --- |
| Real host run before inspection | Succeeded; artifact accepted after confirmed stop |
| Explicit cold starts | Same sandbox ID; three distinct observed runtime generations |
| Checkpoint and original guest output | Retained |
| Guest boot ID after restart | Changed |
| Exec while stopped | Refused |
| Old delayed process after restart plus 13 seconds | No late file |
| Effective policy | Unchanged |
| Guest output changed during inspection | Host artifact and receipt remained byte-identical |
| Final cleanup | Same owned VM confirmed stopped |

The accepted artifact hash was
`e49c81e2d2f84e259d40e2fb8192f3bcd198b355184845d76d8f58807d0d78ee`.
No private machine identifiers, raw logs or configuration are included here.

Earlier setup attempts are retained privately rather than counted as passes:
a noncanonical operator path was refused before a run; missing launch signing
refused provisioning and produced no accepted artifact; an unset runtime data
directory caused launcher failure, followed by confirmed stop and no artifact.
After explicit operator configuration correction, the successful test used a
new run and evidence directory. No production retry or weaker policy was added.

## Next decision

Retain OpenShell as an optional local execution backend, with cold workspace
restart as a qualified backend primitive rather than a new host resume feature.
Before adding host-directed continuation, define explicit new approval,
checkpoint validity, run/generation linkage and output acceptance. Never reopen
an old receipt or silently promote a late guest artifact.

If memory-preserving suspension or consistent VM backups become necessary,
evaluate maintained backend support against concrete acceptance cases then.
Do not introduce a second VM state machine, a private-disk copier or a generic
hypervisor abstraction to fill an interface that is currently absent.

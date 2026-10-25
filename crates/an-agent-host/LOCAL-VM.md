# Local VM direction and capability limits

Status: local-first design boundary, not a new VM implementation. Native
stdio/file execution and the optional [OpenShell adapter](OPENSHELL.md) remain
the implemented profiles. Do not infer general microVM lifecycle support from
their bounded execution acceptance.

The isolation and lifecycle unit is a spool execution attempt. Internal silks
run within its effective authority, without one VM per silk. The bounded
single-entry profiles do not yet implement multi-silk execution. See
[the spool boundary and migration](SPOOL-BOUNDARY.md).

## Ownership

- Loom develops, checks and packages workflow inputs. It does not operate VMs.
- The host approves package/input identities, context disclosure, mounts and
  budgets; it accepts artifacts and retains receipts.
- A qualified backend enforces isolation and owns actual VM/process state.
  The host requests operations and records their evidence.
- Silks compute inside their owning spool boundary. Shuttles mediate separately approved
  operations in user applications; they are not a way around VM isolation.

Reuse a maintained backend rather than building a hypervisor or adopting a
distributed control plane merely for future compatibility. Locality reduces
deployment scope, not the need for isolation, credential separation or settlement
evidence. A lightweight VM is a requirement to evaluate, not a security property
that follows from its name.

## Separate lifecycle capabilities

| Capability | Required meaning | Current status |
| --- | --- | --- |
| Launch and execute | Bound package, input, environment and policy; no weaker fallback | Implemented in bounded native and optional OpenShell profiles |
| Assemble a workspace | Expose only approved inputs, dependencies and writable outputs | Fixed profiles only; arbitrary mounts or stage assembly are not implemented |
| Cancel or stop | Confirm termination, retain uncertain outcomes and reject late success | Existing process supervision and OpenShell stop-settlement checks |
| Cold workspace restart | Boot again from retained disk, without continuing old processes | OpenShell backend primitive; opt-in operator test only, not host-directed resume |
| Pause scheduling | Admit no new work; define what already-running work may do | Not a general host operation |
| Pause a VM | Backend-confirmed execution freeze, distinct from cancellation | Not implemented |
| Suspend and resume | Preserve execution state with a documented restore contract | Not implemented |
| Back up and restore | Identify disk/artifact versions and a consistent restore point | Retained artifacts are not a VM backup or crash-recovery guarantee |

A disk snapshot is not a memory checkpoint. Neither reverses a shuttle operation,
message or model request outside the VM. Backup consistency, encryption, secret
retention and restore compatibility need independent acceptance.
See the [OpenShell lifecycle assessment](OPENSHELL-RECOVERY.md) for source
evidence, the bounded cold-restart acceptance and the remaining capability gaps.

## Next evidence, not a generic framework

Extend only the next concretely needed backend operation. Before claiming it,
exercise success, refusal, cancellation/deadline, control loss and cleanup.
Bind evidence to the backend version, policy and run identity. Separate operation
request acceptance from completion; do not retry an unknown outcome automatically.

A resumed run must revalidate grants, context audience, input revisions and
credential availability. Old process output must not become a new run's accepted
artifact. These are requirements for future resume support, not an implementation
claim. Existing OpenShell publication remains gated on success and confirmed stop.

macOS is the primary local platform. Preserve native Linux code and CI without
assuming that Linux process confinement qualifies a VM backend or that snapshots
are portable across architectures. Optional developer VMs live under repository
`infra/`, outside the host runtime.

## SPACE retirement and retained reuse

SPACE runtime compatibility is deferred. The unused `space-preflight` CLI and
public `space_preflight` module, plus their preparation guide, are removed.
They only observed prerequisites; no deployed SPACE execution capability is
being replaced. Consumers of that diagnostic must stop invoking it. Historical
preparation and acceptance records remain available in Git and infrastructure
documentation; they are not the current implementation roadmap.

The separately locked SPACE-based architecture checker remains a development
dependency. Its TOML rules and non-growing baseline are useful independently of
any runtime backend. This change does not remove that checker, relax CI, alter
native/OpenShell policies or add a new dependency to core.

The first module separation moves host-owned execution identity and settlement
into `execution/environment.rs`; backend mechanics remain behind the existing
contract. This is a code boundary, not a new deployable environment service.
Locality, guest OS and isolation type are separate future capability dimensions.
A process sandbox, container and VM must not advertise interchangeable pause,
restore or isolation guarantees. No second backend is introduced by this split.

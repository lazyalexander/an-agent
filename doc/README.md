# an-agent documentation

This is the public documentation starting point. It distinguishes the accepted
product direction from the bounded profiles actually implemented. It contains
no machine configuration, credentials or private operational transcripts.

## Start here

1. [Architecture and vocabulary](architecture.md): host, spool, silk, shuttle,
   Loom and replaceable execution backends.
2. [Execution and authority](execution.md): admission, lifecycle, semantic
   assessment, termination evidence and external application access.
3. [State, artifacts and history](state-and-artifacts.md): committed business
   state, immutable products, scratch, tape and backend snapshots.
4. [Implementation status and next steps](status.md): what works, what remains
   operator-only and the next bounded refactoring slices.

## Detailed implementation contracts

Keep detailed behavior close to the code that owns it. These existing documents
remain authoritative for their individual profiles; this index does not replace
them or turn a design requirement into an implemented guarantee.

| Area | Reference |
| --- | --- |
| Host execution and embedding | [Host README](../crates/an-agent-host/README.md) |
| Spool boundary and migration | [Spool boundary](../crates/an-agent-host/SPOOL-BOUNDARY.md) |
| Permissions and settlement | [Host architecture](../crates/an-agent-host/ARCHITECTURE.md) |
| Platform-specific enforcement | [Security layers](../crates/an-agent-host/SECURITY-LAYERS.md), [platforms](../crates/an-agent-host/PLATFORMS.md) |
| Multi-silk implementation and stress plan | [Native round lifecycle and custody](../crates/an-agent-host/MULTI-SILK-PLAN.md) |
| Native process ownership | [Process lifecycle](../crates/an-agent-host/PROCESS-LIFECYCLE.md) |
| Optional OpenShell backend | [Adapter contract](../crates/an-agent-host/OPENSHELL.md), [recovery limits](../crates/an-agent-host/OPENSHELL-RECOVERY.md) |
| Reviewed operator workflows | [Workspace jobs](../crates/an-agent-host/WORKSPACE-JOBS.md) |
| CLI handoff pressure and input references | [Measurements and bounded decision](../crates/an-agent-host/HANDOFF-PRESSURE.md) |
| Artifact revisions and human edits | [Artifact versions](../crates/an-agent-host/ARTIFACT-VERSIONS.md) |
| Instance state and delivery | [Instance commits](../crates/an-agent-host/INSTANCE-STATE.md), [checkpoint/delivery](../crates/an-agent-host/CHECKPOINT-DELIVERY.md) |
| Delivery authority | [Registration currentness and host landing](../crates/an-agent-core/DELIVERY-CURRENTNESS.md) |
| Tape writer ownership | [Exclusive ownership and recovery](../crates/an-agent-core/TAPE-OWNERSHIP.md) |
| Environment input identity | [Committed environment generations](../crates/an-agent-core/ENVIRONMENT.md) |
| Model/context services | [Model/context](../crates/an-agent-host/MODEL-CONTEXT.md), [model CLI](../crates/an-agent-host/MODEL-CLI.md) |
| Semantic assessment | [Semantic crate](../crates/an-agent-semantic/README.md) |
| Repository gates | [Architecture gates](../ARCHITECTURE-GATES.md) |

Documentation is English. Link to existing contracts instead of duplicating CLI
reference material. Update status when a slice gains real acceptance evidence.
Historical profile names and records remain stable unless a separately reviewed
migration changes them. Do not copy private deployment notes into public docs.

Code-role terminology follows [the spool code contract](spool-code.md).
Silk is the stable core; kit is adaptable glue. Preserve legacy wire/API names
and distinguish target contracts from implemented runtime guarantees.

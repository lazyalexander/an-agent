# an-agent documentation

Architecture and operational contracts live here. Package and directory READMEs
provide orientation and links, not parallel specifications. Commands assume the
repository root unless stated otherwise. Current guarantees are bounded by the
[implementation status](status.md) and [platform matrix](platforms.md).

## Product and development

| Topic | Reference |
| --- | --- |
| Responsibilities, authority and dependency direction | [Architecture](architecture.md) |
| Admission, supervision and backend boundaries | [Execution](execution.md) |
| State, products, scratch and history | [State and artifacts](state-and-artifacts.md) |
| Stable silk and editable kit roles | [Spool code](spool-code.md) |
| Reviewed harness development | [Weaver](weaver.md) |
| Implemented capabilities and remaining work | [Status](status.md) |
| Runtime support and qualification | [Platforms](platforms.md) |
| Session coordination and remaining retention work | [Session and retention plan](session-and-retention.md) |
| Testing, CI and cross-repository ownership | [Testing](testing.md) |
| Code and documentation quality gates | [Quality](quality.md) |

## Runtime contracts

| Task | Reference |
| --- | --- |
| Load/run/edit, round control, Python environment selection | [Native spool](native-spool.md) |
| Select/commit explicit state and compare releases | [Native state](native-state.md) |
| Observe, route and dispatch fixed-version events | [Native events](native-events.md) |
| Private collection, input retirement and reference release | [Native storage](native-storage.md) |
| Register and execute a bounded stdio deployment | [Native deployment](native-deployment.md) |
| Reviewed builds/workspaces and process ownership | [Execution profiles](workspace-jobs.md) |
| Immutable product versions, input clones, mediated reads | [Artifacts](artifacts.md) |
| Legacy instance commits and checkpoint recipients | [Checkpoints](checkpoints.md) |
| Model CLI, explicit context and finite coding-tool access | [Model services](model-services.md) |
| Optional backend setup and qualified recovery limits | [OpenShell](openshell.md) |
| Embedded delivery, tape and environment identity | [Core contracts](core-contracts.md) |
| Optional developer setup and historical qualification | [Qualification recipes](qualification.md) |

Update the owning contract and status together when behavior changes. Link rather
than repeat commands, support matrices or safety guarantees. Completed plans and
obsolete architectural alternatives are removed; Git history retains the original
rationale and measurements. Private machine configuration and raw operational
records do not belong in public documentation. Historical profile and wire names
remain compatibility identities.

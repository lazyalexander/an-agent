# Platform boundaries

Three independent targets must not be conflated:

| Surface | Target and authority |
| --- | --- |
| Control host | Native an-agent binary, authorization, context and file custody |
| Shuttle | The target application's OS, API/GUI and permissions |
| Workflow execution | The admitted workload's OS/architecture, dependencies and backend |

Build separate binaries for each OS/CPU target. A Rust target also needs its
appropriate linker and native dependencies. Shipping a binary does not port
sandbox semantics, GUI APIs, drivers, application availability or credentials.
Keep protocol boundaries explicit; do not assume arbitrary Rust dynamic libraries
form a portable plugin ABI.

Current qualification is narrower than intended distribution: native stdio/file
kits have macOS and Linux CI; reviewed workspace development jobs are macOS-only.
Windows and arbitrary Linux workspace jobs are not qualified by these examples.
The finite MCP broker reuses the native file path; paid Kimi acceptance currently
runs on macOS. The coding client is trusted software on the control host.

The authoring default for future workflow deployments is CPU-only Linux, with an
explicit architecture and toolchain. This is a deployment policy, not a claim that
every existing macOS development example already runs inside Linux. A local ARM
Linux VM can provide a CPU development target; native Linux must not depend on
Lima or Docker. No new VM integration is introduced by this change.

GPU and application-specific requirements are explicit alternative profiles.
Possible routes are an authorized remote GPU/model service, a Linux GPU worker,
or a native application shuttle on the control machine. Do not promise GPU
passthrough from a local VM. Remote work needs scoped credentials/network access,
bounded artifact exchange, cancellation confirmation and rejection of late results.
These routes are architecture options, not newly implemented adapters.

For example, a macOS shuttle may export a selected document revision, a CPU Linux
kit may process it, and a macOS shuttle may apply the accepted result after a
revision check. Neither the workflow nor Loom receives unrestricted GUI access.
An optional GPU service does not require changing this ownership division.

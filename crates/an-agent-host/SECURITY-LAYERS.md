# Host security layers

The host is trusted orchestration, not a complete operating system. A spool's
declaration and static analysis report are evidence, never the enforcement
boundary. No single prompt, shell classifier, or filesystem check is sufficient.

| Layer | Current responsibility | Limitation |
| --- | --- | --- |
| Producer inspection | Waver checks selected sources, packages and declarations | Incomplete analysis; macros/aliases remain unknown |
| Runtime admission | Host revalidates deployment and core admits declared actions | Does not prove arbitrary executable behavior |
| Operator/tool | Explicit run approval, narrow CLI, untrusted-output guidance | Prompts and external tool approval modes are not OS isolation |
| Model connection | Host binds endpoint/model/options and keeps bearer secret | Trusted process; no provider proxy or namespace |
| Context | Explicit destination, digest, bounded user-data clips | Attribution is claimed; prompt injection is not solved |
| Process/files/network | macOS Seatbelt or explicit Linux Docker stdio profile, fixed executable, deny-by-default | Narrow ABI; container engine is trusted; native backend has no resource quota |
| Evidence/files | Private run artifacts, bounded streams, receipts and tape | Disk failure/crash may leave partial evidence; no global rollback |

## Platform boundary

Keep macOS SBPL quoting, dyld root-vnode exception and system-library allowlist
inside the macOS backend. Common pipe draining, input/output limits, timeout and
receipt logic must not encode those platform assumptions. Unsupported platforms
refuse native execution; opt-in Docker tests additionally exercise Linux containment.

The [Linux container backend](CONTAINER-CLI.md) tests persistent-write and network
denial, host-file isolation, timeout/cancellation removal, resource constraints,
protected supervisor state, environment separation and evidence retention.
Runtime image and container metadata remain readable; this is not arbitrary
application confinement. Do not translate macOS path
exceptions into Linux policy. Do not silently run unsandboxed on missing features.
Treat the external code tool itself as a separate workload: sandboxing the kit
does not confine the tool's other commands, hooks, MCP servers or startup plugins.

## OpenShell research, 2026-10-18

NVIDIA's current design separates a trusted supervisor from the workload and
places enforcement below agent decisions. Linux uses filesystem/syscall controls
and an outer network boundary; compute drivers own platform setup. This supports
evaluating an existing Linux backend rather than recreating container machinery.
See [OpenShell architecture](https://docs.nvidia.com/openshell/latest/about/architecture).

The v0.1.4 inference documentation uses provider profiles and per-sandbox
attachments, with credentials substituted only at authorized endpoints. Native
clients retain their request formats. Its old global managed inference route has
been removed; do not build a new integration around that obsolete interface.
See [provider access](https://docs.nvidia.com/openshell/v0.1.4/how-it-works/inference).

This repository does not yet integrate or certify OpenShell. A Linux adapter
should be a separately reviewed experiment with failure tests and a pinned
version, not a claim that the current macOS backend is container isolation.

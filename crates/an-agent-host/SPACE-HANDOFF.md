# Mac control client to Linux SPACE: preparation contract

Status: preparation only. The host has a read-only machine prerequisite probe;
it has no SPACE execution adapter, remote authorization protocol or qualified
microVM profile yet. Existing native stdio/file profiles are unchanged. Do not
interpret a successful diagnostic, TCP connection or package check as acceptance.

## Equipment gate

Do not purchase hardware or start an infrastructure migration before checking an
existing operator-selected machine. Do not discover SSH targets, scan networks,
start Docker, install packages or change hypervisor settings as part of a probe.

Run on each explicitly selected control/worker machine:

```sh
cargo run -p an-agent-host --bin an-agent-host -- space-preflight
```

The probe observes only the local OS/architecture and, on Linux, whether KVM and
TUN character devices can be opened. It does not create VMs, issue device ioctls,
launch commands or make network calls. Exit zero means a report was produced.
`candidate: true` only passes these preliminary checks; `ready` and `authorized`
remain false. Missing device access can mean permissions or an absent device,
not proof that the hardware lacks virtualization. This is a documented deployment
filter, not a statement that all ARM or macOS virtualization is impossible.

The currently selected upstream installation path requires Linux x86_64, usable
KVM/TUN, loop devices, btrfs support and a Docker daemon permitting the documented
privileged setup. A guest host also needs working nested virtualization. Docker
is deployment machinery here, not the sandbox contract. Follow the upstream
installation checks; do not grant an untrusted workload the Docker socket.

An ARM Mac can remain the control/application machine. Its CPU and memory do not
establish compatibility with the x86_64 worker path. Apple lightweight VMs may be
a later backend, but are not a drop-in SPACE worker or equivalent acceptance
evidence. No new Apple backend is introduced here.

An operator-approved local ARM Linux VM is useful for Linux userspace, host CLI
and transport tests without buying hardware. Apple documents nested virtualization
on M3 and later, subject to its runtime capability query, VM configuration and
guest kernel support. This does not turn an ARM guest into an x86_64 KVM worker.
The selected SPACE environment's qualified path is x86_64; ARM runtime downloads
alone do not qualify its full lifecycle. Keep local Linux validation separate
from SPACE acceptance. Do not use slow x86 emulation or translated binaries as
evidence that the required x86 KVM isolation works.

Store endpoint names, hardware inventories, credentials and raw diagnostic
records privately. Public acceptance records should contain sanitized capability
results and source versions, not machine identity. An unavailable Docker daemon
does not establish anything about the capabilities of its stopped Linux guest.

## Minimal acceptance fixture

Use a fixed, std-only Rust CLI kit: read bounded UTF-8 input, trim lines, remove
empty/duplicate lines, sort and emit the result. This matches the existing Waver
file-kit business fixture without adding a new domain. For example, input
`pear\napple\npear\n` must produce `apple\npear\n`. No model, GUI, credentials,
package download or network access is needed by the kit.

Build for the actual Linux guest architecture using an approved toolchain;
never reuse the macOS binary as Linux acceptance evidence. Pin source, program,
toolchain and resolved template identities. Reuse the business fixture, not the
native file-kit enforcement profile as SPACE evidence. The SPACE transport/profile must be
qualified separately before it can become a supported host deployment.

## Ordered gates for the next implementation

1. On an operator-selected Linux machine, pass upstream prerequisites and its
   first-sandbox walkthrough using a recorded SPACE revision and template digest.
   This establishes infrastructure only, not kit isolation or host integration.
2. From the Mac, authenticate to the selected management and data-plane origins.
   Use HTTPS or explicitly configured loopback SSH tunnels. Do not forward tokens
   through arbitrary proxies/redirects; validate command attachment origins and
   keep session tokens separate from management credentials. Never give the kit
   a token that can create sandboxes or expand policy.
3. The host presents an explicit plan binding package, input, guest environment,
   exact command, empty/minimal environment, resource limits and output policy.
   Operator approval is separate from model suggestions. Unsupported limits or
   egress policy block the run. No silent native/local execution fallback.
4. Stage only approved inputs in a fresh sandbox. Start one bounded kit command,
   retain operation/sandbox/session IDs and distinguish accepted creation,
   readiness, process completion and output-stream completion. Do not reuse a
   writable sandbox across unrelated runs in this first slice.
5. Bound transferred output and receive it into a fresh quarantine location.
   Validate declared names, sizes, regular-file/type constraints and byte hashes;
   reject unexpected paths/links and do not extract an unchecked guest archive.
   A transport hash identifies received bytes, not business correctness or
   authenticated remote attestation. Check the fixture result before accepting it.
6. Record the run result and accepted artifact identity on the host. Export a
   bounded context view only by explicit selection; do not blindly reuse the
   current local receipt parser for untrusted remote reports. Publication,
   registration, model calls and applying to user software remain separate.
7. Request sandbox deletion and confirm settlement. Retain recovery identifiers
   on failure; do not claim cleanup from an HTTP acceptance or lost connection.
   Do not automatically rerun a command after an unknown outcome.

SPACE owns VM placement/lifecycle and process enforcement; an-agent owns the
workflow and artifact decisions above. Reuse upstream endpoints and lifecycle
operations rather than copying SPACE controllers. Waver exports checked package
bytes and does not gain a remote executor or a dependency on SPACE.

## Required evidence before claiming a working loop

| Case | Required observation |
| --- | --- |
| Normal run | Expected bytes, successful process result, bounded accepted artifact, confirmed cleanup |
| Wrong architecture / absent KVM | Refusal before execution; no weaker backend fallback |
| Timeout / cancellation | Termination settlement distinguished from request acceptance; no artifact accepted as success |
| Control/data connection loss | Unknown or recoverable state with IDs retained; no automatic command replay |
| Unexpected/oversized output | Rejected reception; no overwrite of existing host files |
| Forbidden network / undeclared host file | Enforcement denial, not just a model promising not to access it |
| Expired or wrong-scoped token | Refusal without leaking credentials into reports or packages |

Do not claim process cleanup, retention automation, crash-atomic reception,
snapshot portability or reversal of shuttle effects until separately tested.
This checklist deliberately stops short of a generic workflow engine.

## Upstream references

- [SPACE installation](https://github.com/perplexityai/SPACE/blob/main/docs/installation.md)
- [SPACE architecture](https://github.com/perplexityai/SPACE/blob/main/docs/architecture.md)
- [First sandbox and command evidence](https://github.com/perplexityai/SPACE/blob/main/docs/first-sandbox.md)
- [SPACE environment qualification](https://github.com/perplexityai/SPACE/blob/main/docs/execution-environments.md)
- [Apple nested virtualization capability](https://developer.apple.com/documentation/virtualization/vzgenericplatformconfiguration/isnestedvirtualizationsupported)

The preparation was checked against upstream revision
`57c7c0c47a73373a974c5b542d180599795465da`. Recheck prerequisites and endpoint
contracts when selecting a different deployment revision.

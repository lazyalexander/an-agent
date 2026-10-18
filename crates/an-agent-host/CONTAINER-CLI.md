# Controlled Linux stdio kits

`linux-docker-stdio-v1` is an explicit host backend, not a package capability or
a Waver runtime. It reuses deployment verification, registration, AgentControl
admission, context digests, receipts, and applied evidence. Native execution on
unsupported operating systems still fails closed; Docker is never an implicit
fallback. The macOS Seatbelt backend is unchanged.

## Operator setup

Install a trusted Docker CLI and local Linux engine (Docker Desktop on macOS,
or a separately administered engine on Linux). The host requires working memory,
swap, PID and seccomp support. Engine access is privileged host authority; never
give its socket or configuration to a kit. This backend accepts local Unix
endpoints only, not remote daemons or implicit Docker contexts.

Build the small runtime explicitly, outside execution:

```sh
docker build -t an-agent-stdio crates/an-agent-host/containers/stdio
docker image inspect an-agent-stdio --format '{{.Id}}'
```

Save an operator-owned JSON profile, substituting the actual full image ID:

```json
{
  "schema": "an-container-profile-v1",
  "docker": "/usr/bin/docker",
  "endpoint": "unix:///var/run/docker.sock",
  "image": "sha256:REPLACE_WITH_64_LOWERCASE_HEX_DIGITS"
}
```

On macOS the CLI and socket paths differ; obtain the endpoint from the chosen
Docker context explicitly. The host clears inherited Docker environment and uses
an empty per-run Docker config directory. It never pulls images. Images must be
Linux amd64/arm64, with no volumes, build hooks, or extra environment entries.
Only PATH, HOME and HOSTNAME entries are accepted and all three are overridden.
Health checks and init injection are disabled. The image filesystem is trusted
operator input: it must not contain secrets. A digest is identity, not trust.

Build the kit for the engine's architecture as a static Linux ELF, not a native
macOS binary. For a small Rust CLI built on Linux, `rustc -C
target-feature=+crt-static` is sufficient for the acceptance sample. General
glibc-dependent programs may still require runtime libraries; this is not a
general application image contract. The host rejects ELF interpreter segments
and architecture mismatches before starting the kit. Waver's existing export
and declarations need no container-specific fields.

```sh
an-agent-host register DEPLOYMENT STORE
an-agent-host run STORE RELEASE INPUT --container PROFILE
an-agent-host run STORE RELEASE INPUT --container PROFILE --execute
```

Preview does not contact Docker or start a container. Its random run identity
changes on execution; `--execute` is an explicit trusted-caller grant, not a
digest-bound remote authorization token. Inspect the executed plan as well.

## Enforcement and evidence

- One executable bind mount, read-only; no store, workspace, source directory,
  model credentials or engine socket mounts. Only bounded stdin crosses in.
- UID/GID 65534, read-only root, no capabilities, no-new-privileges, no network,
  no shared IPC, one PID, 128 MiB memory with no additional swap, one CPU.
- A host-owned seccomp allowlist restricts this static stdio ABI: no sockets,
  process creation, file creation/truncation, mount, ptrace or io_uring. It is
  intentionally less compatible than Docker's general default policy.
- OCI startup needs to reopen an existing FIFO for writing. This policy allows
  that operation; read-only mounts protect persistent files. Standard output
  and existing device nodes are not claimed to be immutable files. Runtime
  image files and the container's limited runtime metadata remain readable.
- Input is limited to 8192 bytes; each retained output stream to 65536 bytes.
  Execution has a five-second wall deadline (including attach/start), and a
  five-second CPU rlimit. Docker management operations each have a separate
  20-second deadline. No daemon logging of unbounded kit output is enabled.
- Timeout, output overflow and CLI SIGINT/SIGTERM lead to force-removal of the
  exact named container, not just termination of its client. Embedders can set
  the supplied atomic cancellation flag without installing process-wide signals.
- `container-cleanup.json` records removal confirmation. Cleanup failure is
  refusal, never success/applied; inspect the recorded name before retrying.
  SIGKILL, machine failure or unavailable Docker can still leave a container.
  The retained plan names it; do not assume automatic crash recovery. Containers
  never restart automatically. Manual inspection/removal is currently required.

The host verifies the created container's principal, mounts, network, read-only
root and resource settings before starting it, retaining `container-config.json`.
The plan binds the image ID and seccomp digest. Receipts bind input/output bytes
and identify the selected backend and image. Only successful execution followed
by confirmed removal and retained output is marked applied. Local trusted store
ownership remains required; same-user hostile mutation races are not solved.

## Model and context separation

Run `an-agent-host model` outside the kit container using the existing preview
and approval contract. Review its retained `output.txt`, then pass that file to
the independently approved `run --container` command. Compare the model receipt's
`result.response_sha256` with the kit receipt's `context.sha256`. The offline kit
does not need network access or the model profile. Waver may prepare invocation
instructions but does not own either runtime operation.

## Tests and limits

`AN_AGENT_CONTAINER_PROFILE=PROFILE cargo test -p an-agent-host --test
container_execution -- --nocapture` opts into real engine tests on Linux. On
macOS, also set `AN_AGENT_CONTAINER_PROBE` to a static Linux build of
`tests/fixtures/container_probe.rs`. Without the profile the engine test reports
a skip; ordinary tests never silently use a developer's daemon.

Tests cover successful stdin output, forbidden writes, sockets, extra processes,
environment isolation, hidden host files/socket, failure without applied evidence,
timeout, output flood, cancellation, and cleanup receipts. Waver's separately
pinned integration adds model-output-to-kit-input digest continuity. Its model
server is a fixture, not a live provider.

This is a narrow tested execution profile, not hostile-code certification or
VM-grade isolation on native Linux. Docker/kernel/runtime vulnerabilities remain
in scope for platform administration. Do not broaden the syscall list or mounts
to make arbitrary workloads pass without a separately reviewed profile.

References: [Docker execution controls](https://docs.docker.com/engine/containers/run/),
[seccomp behavior](https://docs.docker.com/engine/security/seccomp/), and
[daemon trust boundary](https://docs.docker.com/engine/security/).

# Linux compatibility validation

An isolated Docker Desktop Linux/aarch64 run passed the host suite, including
the text model CLI. The official Rust image was resolved to
`rust@sha256:a24e292b4af7bdee351a31d3a6379972a5652f0ea22a1b6d6883324ac4f5097c`.
The repository toolchain selected Rust 1.98.0. Only a disposable source snapshot
and build directory were mounted; no credentials or Docker socket were mounted.
Public dependency downloads were allowed during compilation.

To reproduce, prepare an explicit source snapshot and an empty build directory,
then mount only those directories. Replace the uppercase paths below with
absolute paths selected by the operator (never a home directory or Docker socket):

```sh
docker run --rm --cap-drop ALL --security-opt no-new-privileges \
  --pids-limit 256 --memory 4g --cpus 4 \
  --mount type=bind,source=SOURCE_SNAPSHOT,target=/src,readonly \
  --mount type=bind,source=BUILD_DIRECTORY,target=/validation \
  -w /src -e CARGO_HOME=/validation/cargo -e CARGO_TARGET_DIR=/validation/target \
  rust@sha256:a24e292b4af7bdee351a31d3a6379972a5652f0ea22a1b6d6883324ac4f5097c \
  cargo test -p an-agent-host --locked
```

The 12 model-client/CLI tests then passed again from the compiled artifacts with
network disabled, read-only root and artifact mounts, UID/GID 65534, all
capabilities dropped, no-new-privileges, 64-process and 512 MiB limits, and a
64 MiB noexec/nosuid temporary filesystem. Loopback HTTP fixtures still work in
the isolated network namespace; external model access was not granted.

This is compatibility evidence, **not a shipped Linux spool sandbox**. Native
kit execution on Linux still fails closed. The Docker build container was not
used as a substitute for runtime admission. No platform-specific macOS exception
was translated to Linux and no core code changed.

Next Linux slice: an explicit host-selected container backend with immutable
image identity, controlled mounts, no workload Docker socket, bounded output,
process-tree cleanup on timeout/cancel, and negative filesystem/network tests.
Evaluate a mature backend before adding one. Provider networking requires a
separate authority from the offline kit. Keep container construction and platform
readiness checks outside Waver and outside core.

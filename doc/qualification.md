# Developer qualification recipes

Current support is authoritative in [platforms](platforms.md). These recipes use
existing enforcement; they do not add Linux compatibility, a VM backend or a
new deployment target. Native headless spool work currently prioritizes macOS.

## Optional ARM Linux development machine

Native Linux users need neither Lima nor Docker. On an Apple silicon developer
machine, review [the Lima configuration](../infra/dev/lima/arm-linux.yaml) and
install Lima explicitly before running from the repository root:

```sh
limactl validate infra/dev/lima/arm-linux.yaml
limactl start --tty=false --name=an-agent-arm-validation infra/dev/lima/arm-linux.yaml
limactl shell --workdir=/home/runner an-agent-arm-validation uname -srmo
```

This is an ordinary development guest, not the isolation boundary of a spool.
The configuration pins its image and disables host mounts, agent forwarding,
dynamic port forwarding and containerd. Verify these settings in the actual
instance. Existing instances retain copied configuration; do not recreate or
restart one merely because repository files moved.

Install the compiler, linker, OpenSSL development package, Git, Python and the
repository-pinned Rust toolchain in the guest. Review installers and dependency
build scripts as trusted developer actions. Transfer only Git source snapshots
and explicit lockfiles into fresh directories; record revisions and archive
hashes. Do not transfer credentials, private homes or unrelated files. Build
native guest binaries; a Mac executable is not a Linux executable.

Run as a non-root user with the repository toolchain on PATH:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo build -p an-agent-host --bin an-agent-host --locked
```

Landlock ABI 3 or newer, seccomp and required filesystem operations must be
available. Missing enforcement refuses; do not disable restrictions to turn a
refusal into a pass. Existing stdio/file execution tests do not qualify native
workspace jobs, isolated Linux source builds, GPU, SPACE or general microVMs.
Loom paired tests select explicit compatible host revisions; use its current
acceptance map rather than the retired Waver command examples.

Keep receipts, exact source/binary identities, selected inputs, termination
confirmation and accepted artifact hashes. Context export is a separate bounded
selection; it does not invoke a model or authorize another run. Copy back only
explicit evidence files, compare transport hashes, and never follow guest paths
as trusted host paths. Copied receipts are not authenticated remote attestations.
Stop only the development instance owned by the operator, after retaining evidence.

## Historical evidence

The detailed ARM Linux runs, the cancellation/next-launch refusal investigation,
OpenShell cold-restart timings and multi-silk pressure experiments remain in
[the pre-consolidation source tree](https://github.com/lazyalexander/an-agent/tree/b44b3c538eb9428975d38e7cbc3669ddf9b0f827).
Those dated measurements are not current platform certifications or performance
SLAs. Raw logs, machine configuration and provider credentials remain private.

The important product fix from the launch-refusal investigation is preserved:
a host fork could inherit a writer on an executable inode and cause Linux
`ETXTBSY`. Host-owned executable publication now uses a supervised fixed copy
helper, separate data/candidate inodes, exact verification and no-overwrite
publication. Isolating tests or retrying launch was not the fix. See
[the deployment contract](native-deployment.md) and the actual regression in
[execution concurrency](../crates/an-agent-host/tests/execution/concurrency.rs).

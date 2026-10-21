# Native ARM Linux VM acceptance

This is a developer validation environment, not a SPACE adapter, production
microVM profile or multi-tenant security certification. The native host enforces
Landlock/seccomp inside the Linux guest. Lima supplies an ordinary Linux machine;
it is not a runtime dependency or part of spool declarations.

## Reproduce the environment

On an Apple silicon Mac with a supported Virtualization.framework, install Lima
explicitly, review [the VM configuration](validation/arm-linux.yaml), then run:

```sh
limactl validate crates/an-agent-host/validation/arm-linux.yaml
limactl start --tty=false --name=an-agent-arm-validation crates/an-agent-host/validation/arm-linux.yaml
limactl shell --workdir=/home/runner an-agent-arm-validation uname -srmo
```

The configuration pins the image digest, uses four vCPUs, 8 GiB memory and a
32 GiB sparse disk. Plain mode disables mounts, dynamic port forwarding and
containerd. SSH agent forwarding and loading unrelated host public keys are
disabled. Nested virtualization is not needed or enabled. The operator's guest
account may install dependencies with sudo; host/kit execution tests run as the
non-root `runner` account, never under sudo. This is not a VM for giving an
untrusted agent an unrestricted privileged shell.

Verify that there are no virtiofs/9p/SSHFS host mounts and no forwarded SSH agent.
Lima's scoped management SSH connection remains necessary. The guest can reach
the network for trusted dependency installation; this does not grant network
access to a sandboxed kit. Optional download proxy settings must be explicit,
transient and absent from kit plans. Do not put private proxy endpoints in this
repository.

Install `build-essential`, `pkg-config`, `libssl-dev`, `ca-certificates`, `curl`,
`python3` and `git` inside the guest. Install the repository-pinned Rust toolchain
from the official distribution, verifying its installer checksum. No Docker
daemon is required. Use the checked-in lockfiles; compilation of reviewed source
and dependency build scripts is outside the kit runtime sandbox.

Transfer only Git source snapshots with `git archive` and `limactl copy` into new
guest directories. Do not mount the checkout, transfer `.git`, copy credentials,
or share the user's home directory. Record the source commits and archive hashes
privately. Build the host and Waver natively for aarch64; do not copy Mac binaries.

## Execute the real tests

Inside the guest, with the fixed toolchain on PATH:

```sh
cd /home/runner/an-agent
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo build -p an-agent-host --bin an-agent-host --locked
cd /home/runner/an-waver
cargo build --locked
AN_AGENT_HOST_BIN=/home/runner/an-agent/target/debug/an-agent-host cargo test --test host_handoff --test file_job_handoff --locked
```

The real execution test now also verifies retained output -> explicit scoped
context -> a separately requested second kit run, exact downstream input hashes,
wrong-audience refusal and tamper rejection. Selection alone does not execute
anything. Existing containment/lifecycle tests cover denied operations, timeouts,
cancellation and process reaping. Never skip or weaken a failing sandbox test.

Waver's cross-repository model exchange uses a local HTTP fixture, not a live
provider or model quality benchmark. Its Linux file-job test is a refusal test:
Linux file-kit/build execution remains unsupported. A green suite must not be
reported as acceptance of that unsupported profile.

## Retained CLI handoff

Use Waver's existing `templates/cli-text` starter via `init-spool`, explicitly
compile it, then `declare`, `pack`, `export-spool` and host `register` into fresh
directories. For input `  alpha  \n\n beta\n`, the expected output is
`alpha\nbeta\n`. The kit trims lines and preserves order; it does not sort or
deduplicate them.

1. Create an explicit context packet and verify the intended audience. Verify
   that another audience is refused.
2. Preview host `run`; require `linux-landlock-seccomp-stdio-v1`, empty environment
   and no created run. Execute only by a separate explicit `--execute` call.
3. Check successful status, reaped process, expected stdout, input hash and audit.
4. Select that run's `stdout` with `context-export` into a new directory. Verify
   its context packet and confirm that selection did not create another run.
5. Explicitly choose the selected text as a new input and execute a second run.
   Its input hash must equal the first run's selected output hash.
6. Copy only an explicit bounded evidence set back to a fresh Mac directory:
   output, context packet, selection and the two receipts. Compare transport
   hashes and validate the context audience on Mac. Do not extract an arbitrary
   guest archive, follow receipt paths on the Mac, or treat a copied receipt as
   authenticated remote attestation.
7. Confirm modified retained output cannot be re-exported as valid context.

These are retained stdio products and selected context, **not** the file-kit
profile's accepted `artifact.txt`. Applying output to a user document, calling a
model, publishing a package and registering new code require separate authority.

## Stop without losing evidence

```sh
limactl stop an-agent-arm-validation
limactl list
```

Keep the stopped disk for inspection and reruns. Do not delete the VM or its
receipts automatically. It can be started again explicitly; tests should use
fresh run/output directories. Public reports should contain sanitized results,
not host paths, machine names, credentials or private raw logs.

SPACE's Linux/x86_64 KVM path remains a separate acceptance target. Successful
ARM Linux testing neither qualifies that deployment nor requires modifying SPACE.

## Recorded acceptance: 2026-10-22

Validated host source: `b7ef851b3a847495ffe7e882c2cee71d5eb4c0f0`.
Waver source: `5a25caceb90fa3e94b2d2b6c1c30507e438211ec`.
Later documentation-only commits do not change these tested binaries.

Environment: Lima 2.2.1 using VZ/plain mode; Ubuntu 24.04 minimal aarch64;
Linux `6.8.0-146-generic`; Rust 1.98.0. Landlock reported ABI 4. The outer guest
test process reported `Seccomp: 0` and `NoNewPrivs: 0`, so an outer container
filter was not substituting for the host's own policy. Host execution installed
the restrictions in the kit child. Tests ran as a non-root guest user.

| Check | Observed result |
| --- | --- |
| Host workspace tests | 282 passed; zero failed or ignored |
| Host clippy / formatting | Passed, warnings denied |
| Waver cross-repository tests | Five passed, with the real separately built host CLI supplied |
| Waver ordinary tests / clippy / formatting | Passed |
| Retained CLI scenario | Two successful native Linux kit runs; distinct receipts; matching handoff digest |
| Explicit context boundaries | Wrong audience refused; selection did not start a run; changed stdout rejected |
| Return to Mac | Five named evidence files matched transfer hashes; Mac CLI validated context and refused wrong audience |
| VM lifecycle | Stopped after verification; disk and private evidence retained |

Returned output was exactly `alpha\nbeta\n`, SHA-256
`e49c81e2d2f84e259d40e2fb8192f3bcd198b355184845d76d8f58807d0d78ee`.
The cross-repository model exchange used a loopback HTTP fixture, not a real
provider. Linux file execution remained a refusal case. No core, Waver runtime,
SPACE code or deployed permission profile needed changing.

An initial dependency download received HTTP 502; installation was completed
with the official HTTPS Ubuntu source and normal signature verification. A
temporary operator download proxy was used only for dependency acquisition;
the acceptance commands ran offline with no proxy environment. No signature,
Landlock, seccomp or process-cleanup check was disabled to obtain these results.
Raw paths and local logs are deliberately excluded from this public record.

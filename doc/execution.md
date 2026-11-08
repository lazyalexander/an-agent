# Execution and authority

A managed production attempt must identify its owning spool release, selected
inputs/state, effective grant and backend. Internal silks run within that
boundary. A silk call is not a default request for another VM. Developer tools
may invoke standalone programs, but that alone is not a managed spool execution.

## Host-owned flow

1. Validate immutable release/input identities and the requested operation.
2. Assess declarations and relevant current facts without trusting them as grants.
3. Resolve an operator-authorized plan and an adequate execution profile.
4. Record operation intent before submission. Distinguish host operation identity
   from backend environment identity and runtime generation.
5. Execute inside the selected boundary. Internal domain sequencing remains in
   the spool; external capabilities require the host-authorized interface.
6. Reconcile actual results, cancellation state, environment identity and terminal
   settlement. Validate declared candidate outputs before publication.
7. Commit official outputs/state through their supported host contract and retain
   evidence. Reclaim eligible scratch only after the required settlement checks.

The existing host backend contract implements part of this flow for Native, OpenShell and
SPACE single-entry deployments. It is internal, not a stable general plugin
API. Reviewed workspace jobs are a separate trusted operator profile; they do
not yet carry full production spool authority or hostile-agent containment.

## Semantic assessment and hard limits

The semantic plane detects and reports findings. It does not schedule work,
allocate VMs, cancel execution or contact a supervising agent. The host/control
layer decides what to do with findings and requests review when necessary.
Execution boundaries enforce permissions independently of model judgment.
A declaration, passing policy check or recorded receipt is not itself enforcement.

## Cancellation, failure and uncertainty

A cancellation request, CLI exit and confirmed environment termination are
separate observations. A silk subprocess exiting does not prove that sibling or
escaped workers stopped. The target whole-spool boundary must account for all
owned execution before settlement or cleanup is declared complete.

Current profiles have different qualified scopes. Native reviewed workflows
require cooperating workers that do not daemonize or escape their process group.
OpenShell additionally requires stop confirmation for the same owned environment
and generation. Neither should be generalized to unsupported workloads.

Unknown submission or termination is retained for reconciliation, not automatically
retried. A timed-out/cancelled attempt cannot publish a late result as success.
Host crashes, external effects and remote workers require evidence beyond the
local process exit. Do not infer durable crash recovery from an operation journal.

## Backend and application boundaries

OpenShell is optional; Native profiles use their existing OS-specific restrictions.
OpenShell expansion is paused while native reviewed headless lifecycle and custody
are completed. Preserve the adapter and its failure tests without making it a
default runtime dependency. A selected backend must meet the required isolation;
missing support refuses, with no automatic downgrade. Passing Loom checks or
running headless does not make generated code a cooperating trusted worker.
Isolation, credentials, networking, quotas, pause and snapshots are independent
capabilities. Upstream backend support is not proof that our adapter exposes or
has qualified it. Do not add one backend per internal silk.

User applications remain outside the spool environment unless explicitly placed
inside it. A shuttle mediates authorized external reads/changes with selected
versions and actual-effect receipts. Filesystem backup cannot undo a message,
remote API call or arbitrary GUI operation.

Detailed contracts: [host execution](../crates/an-agent-host/README.md),
[security layers](execution.md),
[process ownership](workspace-jobs.md),
[OpenShell](openshell.md).

## Concurrent native attempts

The host owns lightweight attempt coordination; it does not implement a new
container runtime. Native execution is the first local backend. Container or
microVM support remains an independently qualified backend choice, not a reason
to move artifact custody or domain sequencing into that backend.

The fixed native file profile now allows independent attempts in the same host
store. The store lock protects release resolution, input snapshots and exclusive
run-directory allocation, then is released before admission and process launch.
Each attempt owns its directory, AgentControl thread, operation identity, child
and cancellation flag. Short shared metadata operations still fail closed on
lock contention; there is no new queue, stale-lock repair or automatic retry.
A running child no longer holds that lock for its entire lifetime.

Settlement evidence, not absence of the store lock, gates cleanup. A new attempt
is an explicit invocation with a new identity, not resumption of cancelled memory.
Business state is selected explicitly. OS threads and process memory belong to
the backend; no portable thread snapshot is promised. Accepted file revisions
carry data between attempts, with small control notifications remaining separate.

The real CLI regression in `tests/file_runtime/concurrent.rs` starts two named
spool releases in one store, cancels one through SIGTERM, reclaims its confirmed
cancelled work and starts a fresh attempt while its peer continues. The peer pins
v1, finishes after v2 publication, and retains v1 output after scratch collection.
This qualifies the existing single-entry file ABI, not arbitrary multi-silk
process trees, hostile same-user processes, persistent environments or VM backup.
Loom still authors, checks and packages releases; it owns none of these runtime
locks, grants, cancellation or retention decisions.

For reproducible local evidence:

```sh
AN_AGENT_ACCEPTANCE_DIR=/absolute/path/to/evidence \
  cargo test -p an-agent-host --test file_runtime concurrent -- --nocapture
```

The report includes relative event times, full plans, operation IDs, child PIDs,
receipts, settlement/audit records, revision bindings and reclamation results.
These synthetic test records can contain local paths: inspect before sharing.
CI retains the report per OS; timing is diagnostic evidence, not a benchmark.

## One reviewed body with two silks

The qualified reviewed-body path runs one coordinator and two internal silk
processes through the existing reviewed workspace profile
([platform qualification](platforms.md)). One process group, attempt and
cancellation owner cover the whole round. A producer hands off an internal file;
a consumer offers a candidate, and only whole-round settlement can authorize host
copy/verification and publication. A zero-exit leader with abandoned members is
failed, not successful after forced cleanup. The release-bound native facade also
qualifies two internal silk steps; neither path grants a separate environment per
silk or moves domain sequencing into the host.

The [implementation and stress plan](testing.md)
separates current regression/soak evidence from future saturation and crash-fault
tests. This is not a general registered multi-silk deployment or a hostile-code
sandbox. Reuse the current authority boundary before designing another package
ABI; Loom remains responsible for development and packaging only.

### Artifact descriptor and tool identity checks

Native file-kit collection opens with `O_NOFOLLOW | O_NONBLOCK`, then checks and
reads the same descriptor as a bounded, singly linked regular file. Descriptor
and ordinary reads share the registered-custody and sidecar raw-path refusal.
Linux reads
its original host-owned output descriptor and separately refuses pathname inode
replacement; it never accepts bytes from a reopened replacement pathname.
Approved Rust builds rehash the compiler and linker before each compiler stage.
A mismatch refuses that stage without an automatic retry. This detects changes
after approval; trusted toolchain directories must still exclude untrusted writers
because a pathname hash is not an atomic executable launch or a full dependency hash.

## Optional SPACE file execution

`an-agent-host run-space PROFILE STORE RELEASE INPUT [--execute]` uses the same
registered file-kit admission, immutable executable publication, operation
reconciliation, cancellation gate and bounded artifact acceptance as `run`.
Without `--execute` it returns a plan. SPACE owns the Firecracker VM, not another
host or artifact store. There is no native fallback or model driver.

The private operator JSON profile has `python`, `python_sha256`, `client`,
`client_sha256`, `token_file`, `api_url`, `data_plane_url`, `template` and
`control_seconds` fields. Paths must be absolute and have no symlinks; use the
canonical interpreter path. `client` selects the official SPACE
`examples/first_sandbox.py` helper, pinned by SHA-256. The implementation was
based on upstream revision `57c7c0c47a73373a974c5b542d180599795465da`, helper digest
`e9884066ef792571a8f4b4e7925785ea3122e09b9a183a15e6f2f28079264ea9`.
Use HTTPS origins or explicit IPv4 loopback tunnels. The nonempty token file
must be private to the operator. Tokens never enter guest arguments or receipts.
The template must be a complete `NAME@sha256:DIGEST` reference; mutable tags are
refused. Control deadlines are 30–180 seconds per phase plus bounded local reap.

This initial profile accepts Linux executables or guest-compatible shebang
scripts up to 256 KiB, input up to 8192 bytes and UTF-8 output up to 65536 bytes.
The selected template needs root Python 3, standard POSIX utilities and the
program's runtime dependencies. The program runs as UID/GID 65534 in a fixed
file ABI: readonly `input.txt`, precreated writable `output.txt`. Release program
bytes and the work directory are root-owned; the guest may use `/tmp` but no host
mounts or management credentials. Egress is denied. Output collection rejects
links, nonregular files and oversize candidates; extras are never imported.

The host persists operation intent before any backend submission. The thin
transport uses the upstream HTTP/SSE helpers, retaining their submission journals
and event transcripts under `space-control`. Every request has a saved key;
uncertain submissions are not replayed. Remote metadata binds host run and
operation, and settlement checks the same sandbox ID, creation time and template
digest. Cancellation stops the local work phase, then independently attempts
owned sandbox deletion. Only a successful deletion operation permits acceptance.
A lost create response can leave no safe cleanup target: record unconfirmed
termination and reconcile the saved request with an operator, never guess an ID.
This is not durable crash recovery or an automatic remote garbage collector.

Protocol fixtures cover the host registration/admission/receipt flow, cancellation
and unknown termination. On 2026-11-07, the public CLI also passed against real
SPACE at the pinned revision: a macOS control host, Linux x86_64 Firecracker seed,
registered shell program, formal eight-byte output acceptance, owned VM deletion
confirmation and changed-client refusal. This qualifies the fixed file profile,
not arbitrary guests or CAD→Krita. Earlier lab-host evidence remains separate.
Keep the prior business path until its real full-chain qualification passes.

Run `python3 crates/an-agent-host/tests/cli/space_backend.py --host HOST --evidence
NEW_DIRECTORY` for the local CLI fixture acceptance. Adding
`--operator-profile PRIVATE_PROFILE` explicitly targets an existing live service
and creates/deletes one sandbox; it never provisions a cloud server. The runner
uses its own profile copy, leaves operator configuration untouched and retains
failed/uncertain operation journals for reconciliation.

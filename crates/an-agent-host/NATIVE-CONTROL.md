# Native control at round boundaries

The first control slice uses a completed spool attempt as its safe point. It
pauses admission between attempts; it does not stop a running process. The spool
author decides how domain work reaches a committed checkpoint. The host verifies
termination, accepted outputs and fixed identities, without interpreting state.
Loom packages the same v1/v2 release; no new release schema or backend is needed.

```text
an-agent-host spool-pause INSTANCE [--approve HASH]
an-agent-host spool-suspend INSTANCE STATE_REVISION [--approve HASH]
an-agent-host spool-resume INSTANCE [--approve HASH]
```

Every transition has a preview and separate approval. A live operation, stale
lock, unknown termination or successful run lacking formal acceptance refuses.
Prepared plans do not count as execution; an interrupted start with no completed
workspace evidence does. Nothing clears a stale lock or retries an execution.

Pause fences new starts, kit/proposal application and state mutations. Inspection,
proposal revocation and existing guarded artifact custody remain available.
Suspension additionally pins the current committed text checkpoint and effective
kit hashes. It retains state, release and evidence; scratch can be explicitly
reclaimed through the existing command after settlement and acceptance.

Resume verifies the saved release, kits, checkpoint and completed-run evidence
before reopening admission. It launches nothing and carries no old execution
approval forward. Continuation still requires an explicit `spool-start-state`
selection and fresh approval. A domain may instead begin separately authorized
new work. No state, program counter, heap or hidden scratch is injected implicitly.

Control generations prevent old start and transition approvals from becoming
valid again after a pause/resume cycle. A transition publishes immutable evidence
before atomically changing the control head. An interrupted publication keeps the
previous mode; explicit repetition of the same approved transition may finish its
publication. Confirmation of an already published transition is a no-op. An
incompatible staged record refuses for inspection, without overwriting history.
This does not establish sudden-power-loss recovery or hostile host-store safety.

`spool-show` reports the mode and generation separately from a run's outcome.
Existing `spool-status` continues to describe execution and custody, independently
of whether new work is paused. Unload remains terminal, preserves checkpoints and
control evidence, and is allowed from a settled paused/suspended instance. A fresh
load uses the existing explicit fixed-state checkout/seed path and restores kit
baselines; it is not automatic instance or process restoration.

Existing instances without a control head begin at active generation zero. The
first approved transition installs the baseline without changing release identity.
Before publishing any stopped admission mode, it atomically upgrades the private
instance record to `an-native-spool-instance-v2`. Older hosts reject that record
instead of silently ignoring control; hosts supporting this feature continue to
read legacy v1 instances. This is an instance-format floor, not a release-format
change or protection against an operator deliberately rewriting the private store.
Missing or inconsistent control evidence after installation fences admission.
The generic process pause/suspend capability flags remain false. Separate
`pause_between_attempts` and `suspend_checkpoint` flags describe this macOS slice.
In-flight cooperative signaling, transparent process suspension, VM snapshots,
GPU/GUI backends and automatic storage collection remain unqualified.

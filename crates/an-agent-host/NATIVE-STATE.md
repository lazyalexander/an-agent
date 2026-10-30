# Explicit state for native spools

Status: qualified for the reviewed headless macOS shell profile. This adapts the
existing artifact revision store, fixed-input workspace snapshots, accepted-output
evidence and per-instance lock. It does not reuse the legacy file-kit instance
registration, interpret business state, add a backend or change a release format.
Existing native v1/v2 packages and ordinary `spool-start` remain compatible.

## Ownership and use

The spool author defines an opaque UTF-8 state document, its domain validation and
one declared output carrying the next state. Loom packages those domain files and
the existing output declaration. an-agent owns explicit selection, version custody,
execution authorization and serialized publication. A state version is not a
process image, automatic restore instruction or authorization for external effects.

```text
an-agent-host spool-state-init INSTANCE STATE_OUTPUT INITIAL_FILE [--approve HASH]
an-agent-host spool-state-show INSTANCE [REVISION]
an-agent-host spool-start-state INSTANCE RUN REVISION SECONDS [--approve HASH]
an-agent-host spool-accept INSTANCE RUN [--approve HASH]
an-agent-host spool-state-commit INSTANCE EXPECTED_REVISION RUN [--approve HASH]
```

Initialize, start, accept and commit require their own preview and exact approval.
The preview never executes a process or changes HEAD. An initial state is explicitly
operator-seeded, not claimed to be a prior accepted run. Initialization selects one
existing output slot; undeclared outputs are refused. State uses the existing
64 KiB text revision limit. The native profile still has one input: `AN_SPOOL_INPUT`
is the selected state document. A domain needing fresh data must include it in an
explicit domain input design; this slice adds no second input or implicit context.
Selected code and the state input must retain distinct snapshot basenames under
the existing native input naming rule.

`spool-state-show` reports HEAD and a fixed committed selection, including its
catalog/path/binding, without dumping state contents. Only HEAD and up to 1023
ancestors are selectable; uncommitted candidates are not state. An older committed
version may be evaluated explicitly, but cannot replace a newer HEAD.

Start records the exact state revision, selected-code hashes and attempt input
mapping. The existing workspace machinery reads that immutable revision into an
attempt-private read-only snapshot. The workload cannot read or write the state
catalog, HEAD, approval records or sibling attempts. A plain `spool-start` with
identical bytes is not a state-bound run and cannot commit through this API.

## Publication and interruption

A commit independently verifies the successful, termination-confirmed run, native
ownership, fixed state input and formally accepted output inventory. It reads the
selected output from the immutable artifact catalog, never current scratch. The
host holds the instance lock, checks the expected HEAD, records an intent, publishes
an immutable child revision and atomically renames HEAD. Two runs from the same
version may finish; only the first distinct commit advances HEAD. A stale commit
returns `conflict` and `automatic_retries: 0`; the host does not rebase or rerun it.
There is no distributed CAS or guarantee against a hostile operator mutating the
private host store.

Failed, cancelled, timed-out, unconfirmed or unaccepted attempts cannot commit.
Reclaiming settled work does not delete accepted outputs, state versions or input
evidence. State commit can therefore follow scratch collection. Unload preserves
read access but fences new state mutations and starts.

If publication stops before HEAD moves, the previous state remains selected;
an orphan revision is not a committed checkpoint. After inspecting any conflicting
staging file, explicitly repeating the same approved **commit** can finish that
publication without executing the spool again. If the same commit is already the
current child of EXPECTED, confirmation returns `already_committed: true` without
rewriting HEAD. Later descendants produce a conflict rather than inferred replay.
This is bounded commit reconciliation, not automatic retry of an unknown execution.
An interrupted initialization leaves `checkpoints.pending` for operator inspection
and is not silently overwritten. Sudden power-loss durability and arbitrary
filesystem corruption recovery are not established by this qualification.

## Fresh environments and limits

Each state-bound run gets a fresh workspace, with no implicit scratch inheritance.
To continue after a fresh load, explicitly checkout one committed version with
`artifact-checkout CATALOG REVISION NEW_FILE`, load the selected release into a
new directory, and initialize its state from that saved file. The new instance
receives a new state history and operator-seed identity. This does not transfer
old approval, authenticate lineage or validate a cross-release migration. Business
compatibility remains the author's responsibility; automatic update/migration is
a separate milestone. Neither internal scratch nor a VM image is required for
the qualified counter continuation.

State lives under `INSTANCE/checkpoints/`: immutable `state.json`, an atomic HEAD,
the existing `catalog/revisions/` layout and commit intents. No automatic version
garbage collection is added. The later retention work must account for checkpoint
parents, selected inputs and downstream references before deleting storage.

[Round-boundary control](NATIVE-CONTROL.md) can fence new starts and state/kit
mutations after settlement. Resume launches nothing and injects no implicit
state. Control generations invalidate earlier execution and state-mutation
previews; request a new preview after resuming. Already accepted artifacts and
checkpoint inspection remain available while admission is paused or suspended.

Real acceptance: [CLI runner](tests/cli/README.md). It includes actual native
execution/cancellation, stale competing commits, a filesystem staging failure
after publication, orphan refusal, explicit commit confirmation, scratch mutation
after acceptance, reclaim-before-commit and a fresh instance with restored kit
baseline. No model, synthetic execution receipt or new host domain rule is used.

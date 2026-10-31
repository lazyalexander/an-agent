# State, artifacts and history

These are different assets with different owners and recovery guarantees.

| Asset | Meaning and ownership |
| --- | --- |
| Release | Immutable domain implementation and dependencies; identified and retained as a package |
| Instance state/checkpoint | Spool-defined business continuation data; host manages identity, versions, commit preconditions and retained references |
| Artifact | Explicitly accepted product bytes, immutable revision and provenance; host custody makes selected versions available downstream |
| Internal work files | Disposable scratch by default, including temporary kits and adapted kit working copies; optional recovery/debug retention is explicit |
| Tape and boundary receipts | Causal/diagnostic history, authority and outcome evidence; not a complete world backup |
| Backend disk or memory snapshot | Backend-specific execution state; optional and distinct from portable business state |

A release plus a sufficient committed checkpoint can support a new attempt in a
fresh environment. The spool defines whether its state is sufficient; the host
cannot certify business completeness from opaque bytes. This does not recreate
old process memory, a missing package, an unavailable model or external effects.

See [the spool code contract](spool-code.md) for immutable silk/package bytes,
editable kit copies, explicit promotion and code-as-data execution limits.

## Publication and consumption

Outputs stay candidates until host acceptance. Human editing uses a separate
working copy; submitting saved bytes creates a new revision with a parent. It
does not overwrite the accepted version. Human-edit provenance is an explicit
operator declaration, not cryptographic proof of authorship.

Downstream attempts bind exact revisions and digests, never a mutable latest
pointer. A newer version does not change running inputs or authorize a rerun.
The existing instance commit checks the expected state head and rejects stale
updates. The optional checkpoint/delivery contract records selected recipients,
state references and committed consumption. Missing acknowledgements alone do
not authorize duplicate execution.

## Retention and interruption

Formal state/artifacts live outside disposable execution scratch. They must
survive eligible scratch collection. Retained state references and unresolved
delivery/execution evidence can prevent product reclamation. A successful
consumer acknowledgement is not by itself deletion authority.

Native accepted outputs can separately close new revision-based input admission.
This preserves immutable payloads, admitted execution/custody and independent
checkpoints; it is not a reference-release receipt or permission to delete bytes.
Private work/snapshot collection remains independent. See [the native storage
contract](native-storage.md).

The fixed native file profile permits explicit reclamation after confirmed
cancellation as well as success. Cancelled partial output is never published or
used as a capability request. Missing settlement or unknown termination refuses
cleanup; failed and timed-out runs remain outside this cleanup policy.

Interrupted attempts retain uncertainty and required diagnostic/recovery data.
Do not collect a directory while possible writers remain. Current reclamation
is bounded and explicit; there is no general periodic collector or complete
power-loss recovery guarantee. Workflow-engine resume caches remain opaque to
the host and require explicit abandonment before their scratch is reclaimed.

A stopped environment is not an independent backup. Disk restart, memory restore,
state migration and undo of external software changes need different contracts.

See [artifact versions](artifacts.md),
[instance commits](checkpoints.md),
[checkpoint/delivery](checkpoints.md) and
[retention/capabilities](artifacts.md).

## Opaque scratch and borrowed inputs

Undeclared internal files are not published products or implicit state. Operational
inventory/limits for safe cleanup remain distinct from business-level indexing.
External read-only access can avoid an extra file copy, but does not freeze a
source that another application can change. Fixed-version inputs need managed
immutable bytes or a qualified snapshot; live inputs must not claim that guarantee.
See the [handoff pressure decision](testing.md).

The proposed [session and retention plan](session-and-retention.md) separates
evidence references, explicit byte-retention pins and independent copies. The
native pin ledger, formal payload collector and session coordinator described
there are not implemented yet.

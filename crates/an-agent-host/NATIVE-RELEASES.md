# Native release review and independent continuation

```text
an-agent-host spool-release-diff PREVIOUS_PACKAGE CANDIDATE_PACKAGE
an-agent-host spool-release-diff INSTANCE/release CANDIDATE_PACKAGE
```

This is a nonexecuting comparison of two independently verified native v1/v2
packages. It creates no instance, approval, state or update relationship. An
instance's `release/` directory selects its immutable baseline, not its edited
kit working copies, current checkpoint or effective grants.

## What the report establishes

The host verifies complete inventories, bounded code bytes, declared hashes,
paths and schema/requirements pairing for both packages. Unknown fields,
unlisted material, changed payloads and forbidden links refuse inspection just
as they refuse loading. Inspection executes no package hook or dependency tool.

The report uses `an-native-spool-release-diff-v1` and includes:

- Exact previous/candidate release identities and their parsed manifests.
- Added, removed or changed code paths with before/after hashes.
- Changes to schema, name, version, entry and output declarations.
- Before/after runtime, target, interaction, GPU, input, editable, capability
  and dependency declarations. A changed list is displayed in full, not scored
  as safe merely because it shrank.
- Current-host runtime support/refusal for each package, separately from byte
  integrity. Unsupported requests remain inspectable but cannot become loads.
- `reused_coordinates` when name/version labels match but exact bytes differ.

The release digest is still the SHA-256 of the exact manifest bytes, transitively
binding its code hashes. Formatting-only changes can produce a different release
without semantic field changes. No existing manifest or release identity changes.
V1 requirements remain absent, with its implicit kit-edit policy explicitly
identified. Missing declarations are never fabricated as equivalent v2 grants.

`review_required` indicates changed exact identity; it is not an authorization
policy. Even identical packages return `execution_granted: false` and
`successor_authorized: false`. Runtime support is not execution approval or proof
of safe domain behavior. The report does not authenticate a publisher, interpret
SemVer, resolve dependencies or establish compatible state schemas. Static
comparison remains available on Linux, without claiming Linux native execution.

## Supported operator path

1. Compare the old retained release and a verified candidate. Review domain
   contracts, permissions, dependencies and any edited kit separately.
2. Load an admitted candidate into a new directory. The existing instance,
   release, kit, accepted products and checkpoint history stay unchanged.
3. If the spool's domain contract allows it, explicitly checkout a fixed old
   checkpoint and initialize the new instance from that file. This creates a
   new independent history; it is not automatic migration or update lineage.
4. Preview and approve execution for the new instance and exact input. Old
   instance/release approvals cannot authorize it. Accept successful outputs and
   commit state only through existing custody and expected-version checks.
5. Preserve the old instance until the operator chooses otherwise. Rejected
   admission or a failed candidate run does not switch its release or checkpoint.

There is no in-place upgrade, publisher authority store, generic state migration
or automatic retry. Domain authors supply compatibility and migration rules;
Loom packages those materials. The host owns admission and runtime custody.
Future authenticated lineage and exact external dependency binding require
separate contracts rather than treating this report as an update certificate.

## Qualification

Rust unit tests cover exact identities, formatting-only differences, legacy
requirements, unsupported requests and byte/schema integrity refusal. The real
macOS CLI suite `tests/cli/native_release.py` reviews same-label changed releases,
refuses extra capabilities/dependencies before load, rejects cross-instance
approvals, explicitly seeds independent state, rejects a failed candidate's
partial output and continues the original instance with its prior approval.
It is not a generic migration or power-loss recovery test. Evidence is retained
privately; no agent/model, new backend or domain rule is added to the host.

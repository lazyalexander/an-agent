# Operator-selected native Python and binary products

Status: qualified for reviewed, CPU-only, headless macOS tools. This extends the
[native v3 event profile](NATIVE-EVENTS.md) through existing workspace execution
and binary artifact custody. It adds no core dependency, environment installer,
package resolver, agent harness, domain parser or extra isolation per silk.

## Portable requests and local selection

A native v3 release may declare `binary_outputs`, a unique subset of `outputs`.
Omission preserves text output behavior. Each binary output is accepted as opaque
bytes through the existing artifact-v2 contract: 8 MiB per file, 16 MiB combined
outputs; text retains its UTF-8/64 KiB bound. Format, formulas, ZIP expansion and
domain checks belong to the consumer. A file extension does not select encoding.
An older v3 host rejects the new field instead of silently accepting it as text.
V1/v2 bytes and behavior remain unchanged; they cannot declare binary outputs.

Requirements may select `macos-native-python-v1` only in v3, with the existing
macOS, portable-source, headless, GPU-none and exact capability request set. One
file input remains supported. Its declared slot name becomes the snapshot name,
so a consumer can receive `source.xlsx` from an artifact stored as `content.bin`.
Code basenames and the input name must be distinct. There is no extension-based
dispatch or implicit state/input merging.

The release carries bounded unique dependency labels, not executable paths,
installation hooks or grants. The operator separately selects a compatible
environment:

```text
an-agent-host spool-load DELIVERY NEW_INSTANCE --runtime RUNTIME_JSON
```

The strict operator JSON has these fields:

```json
{
  "schema": "an-native-python-runtime-v1",
  "executable": "/selected/python/bin/python3",
  "executable_sha256": "EXACT_SHA256",
  "stdlib": "/selected/python",
  "site": "/selected/venv/lib/python3.12/site-packages",
  "site_sha256": "EXACT_INVENTORY_SHA256",
  "dependencies": ["example-1.0.0"]
}
```

Paths must already be canonical absolute selections. `stdlib` and `site` are
separate directories, outside both the release and instance; root-directory,
symlink and nested selections refuse. Dependency labels must match the release
exactly. Shell releases reject an unsolicited runtime; Python releases refuse a
missing binding before creating an instance. Load copies the selected JSON to
read-only `runtime.json`, binds its exact digest in `instance.json`, and executes
no interpreter, project hook or installer. Start still requires its own approval.
Loom never supplies or approves this machine-local binding.

The site inventory is SHA-256 of compact UTF-8 JSON mapping sorted relative POSIX
file paths to SHA-256 content digests. All files, including bytecode, are included.
Symlinks, hardlinks, nonregular files, more than 8,192 entries, depth over 16,
files over 20 MiB or total bytes over 256 MiB refuse. The host does not import
packages while checking them. Labels are operator assertions; neither their
equality nor content hashes authenticate the package publisher or certify safety.

## Execution and acceptance

The existing workspace supervisor runs the selected interpreter with `-S -B`,
attempt-private code/data snapshots and no ambient environment. `PYTHONPATH`
names only the selected snapshot and site directory. User-site initialization,
`.pth` startup processing and bytecode writes are disabled. `HOME` and `TMPDIR`
point into work scratch; selected runtime roots are read-only and networking
remains denied by the reviewed native policy. The operator binding digest is
included in the approved execution environment.

The interpreter digest and dependency inventory are rechecked at load, while
planning an approved start, and before native artifact acceptance. Changed
dependencies refuse instead of silently changing the environment, launching a
replacement attempt or accepting its outputs. Restore/reselect a valid
environment explicitly; an unknown execution is never automatically retried.
The standard library and system dynamic libraries remain operator-owned platform
dependencies, not immutable packaged snapshots. Pre/post checks are not a proof
against a hostile concurrent same-user change-and-restore attack. This is the
existing reviewed-tools boundary, not arbitrary hostile-code containment.

Native artifact discovery and collection depend on retained execution/custody
evidence, not the continuing installation of Python. Accepted binary artifacts
and consumption confirmations remain readable after work/snapshot collection,
instance unload or loss of the runtime. Unaccepted successful work remains
retained until its acceptance boundary can be resolved.

Binary events use the same exact full revision binding as text events. A delivery
selects `content.bin` for verified byte revisions and carries the revision into
workspace input provenance. It never follows a mutable latest pointer. Explicit
acceptance and consumption completion remain separate from successful execution.

## Qualification

```sh
python3 -B crates/an-agent-host/tests/cli/native_binary.py \
  --host /selected/an-agent-host --evidence /private/tmp/native-binary-NEW
```

The real macOS suite checks missing/mismatched runtime refusal, changed dependency
refusal before start and acceptance, read-only runtime enforcement, opaque byte
handoff after a newer publication, confirmed cancellation, active collection and
cancelled-output acceptance refusal, and retained products/duplicate confirmation
without Python.
It records commands and evidence privately and never rebuilds its selected binary.
Loom separately packages the existing log-report code and qualifies real editable
XLSX/PPTX products. No GUI, model, GPU or new backend is claimed.

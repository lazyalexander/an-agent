#!/usr/bin/env python3
"""Real headless artifact admission retirement, retained custody and no replay."""
import argparse
import hashlib
import json
from pathlib import Path
from host_qualification import require_native_workspace
import signal
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    options = parser.parse_args()
    require_native_workspace()
    host, root = options.host.resolve(strict=True), options.evidence.resolve()
    root.mkdir(mode=0o700)
    calls = []

    def spawn(*args):
        return subprocess.Popen([str(host), *map(str, args)], text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def finish(process, success=True):
        try:
            out, err = process.communicate(timeout=40)
        except subprocess.TimeoutExpired as error:
            process.send_signal(signal.SIGINT)
            try:
                process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                raise RuntimeError(f"Termination unknown for {process.pid}; retain {root}") from error
            raise RuntimeError("Client deadline exceeded; no retry") from error
        calls.append({"args": process.args, "exit": process.returncode, "stdout": out, "stderr": err})
        (root / "commands.local.json").write_text(json.dumps(calls, indent=2))
        assert (process.returncode == 0) == success, (process.args, out, err)
        return json.loads(out) if out else None

    def call(*args, success=True):
        return finish(spawn(*args), success)

    def approved(*args):
        preview = call(*args)
        return call(*args, "--approve", preview["approval_sha256"])

    def package(name, consumer=False):
        release = root / (name + "-release")
        (release / "silk").mkdir(parents=True)
        code = ('set -eu\nawk \'{s+=$1} END {print s}\' "$AN_SPOOL_INPUT" "$AN_SPOOL_STATE" > state.txt\n'
                'printf ready > ready.marker\nsleep 12\ncat state.txt > report.txt\n') if consumer else (
                'set -eu\ncat "$AN_SPOOL_INPUT" > batch.txt\nprintf spare > spare.txt\n')
        (release / "silk/main.sh").write_text(code)
        req = {"runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
            "interaction": "headless", "gpu": "none", "inputs": ["batch.txt"], "editable": [],
            "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []}
        if consumer:
            req["state_input"] = "checkpoint.txt"
        (release / "native-spool.json").write_text(json.dumps({
            "schema": "an-native-spool-v3", "name": name, "version": "0.1.0", "entry": "silk/main.sh",
            "files": {"silk/main.sh": hashlib.sha256(code.encode()).hexdigest()},
            "outputs": ["state.txt", "report.txt"] if consumer else ["batch.txt", "spare.txt"],
            "requirements": req, "events": {"consumes": ["artifact.accepted"] if consumer else [],
                                             "produces": ["artifact.accepted"]}}))
        return release

    a, b, c, ws, other = (root / name for name in ["producer", "running", "waiting", "events", "other-events"])
    call("spool-load", package("producer"), a)
    consumer = package("counter", True)
    for instance in [b, c]:
        call("spool-load", consumer, instance)
    for workspace in [ws, other]:
        call("spool-events-init", workspace)
    seed = root / "zero.txt"
    seed.write_text("0\n")
    heads = {instance: approved("spool-state-init", instance, "state.txt", seed)["state"]["revision"]
             for instance in [b, c]}
    route = root / "route.json"
    for name, instance in [("active", b), ("waiting", c)]:
        route.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(a),
            "output": "batch.txt", "target": str(instance), "result": "report.txt", "wall_seconds": 30}))
        approved("spool-event-subscribe", ws, name, route)
    batch = root / "input.txt"

    def produce(name, text):
        batch.write_text(text)
        approved("spool-start", a, name, batch, 15)
        call("spool-artifact-retire", a, name, "batch.txt", success=False)  # Acceptance is mandatory.
        approved("spool-accept", a, name)
        products = call("spool-artifacts", a, name)
        event = approved("spool-event-publish", ws, a, name, "batch.txt")["event"]["id"]
        return products, event

    def start_live(instance, event, subscription):
        args = ["spool-event-deliver", ws, event, subscription, "--state", heads[instance]]
        preview = call(*args)
        run = preview["plan"]["context"]["delivery"]
        process = spawn(*args, "--approve", preview["approval_sha256"])
        deadline = time.monotonic() + 15
        marker = instance / "runs" / run / "job/work/ready.marker"
        while not marker.exists() and time.monotonic() < deadline and process.poll() is None:
            time.sleep(0.05)
        assert marker.exists() and process.poll() is None, (process.args, run)
        return process, run, args

    products, event = produce("first", "2\n")
    output = next(v for v in products["outputs"] if v["path"] == "batch.txt")
    revision = output["binding"]["revision"]
    catalog = Path(products["catalog"])
    folder = catalog / "revisions" / revision
    original = {name: (folder / name).read_bytes() for name in ["content.txt", "revision.json"]}
    saved = root / "independent-copy.txt"
    call("artifact-checkout", catalog, revision, saved)
    waiting_args = ["spool-event-deliver", ws, event, "waiting", "--state", heads[c]]
    waiting = call(*waiting_args)
    waiting_run = waiting["plan"]["context"]["delivery"]
    selection = root / "selection.json"
    selection.write_text(json.dumps({"schema": "an-native-data-state-v1",
        "data": {"catalog": str(catalog), "revision": revision}, "state_revision": heads[c]}))
    direct = call("spool-start-data-state", c, "direct", selection, 30)
    dispatch = root / "dispatch.json"
    dispatch.write_text(json.dumps({"schema": "an-native-event-dispatch-request-v1", "name": "not-started",
        "entries": [{"event": event, "subscription": "waiting", "state_revision": heads[c]}]}))
    scheduled = call("spool-events-dispatch", ws, dispatch)
    process, run, live_args = start_live(b, event, "active")
    retire_args = ["spool-artifact-retire", a, "first", "batch.txt"]
    preview = call(*retire_args)
    assert preview["plan"]["payload_deleted"] is False and not (folder / "admission-retired.json").exists()
    call(*retire_args, "--approve", "0" * 64, success=False)
    call("spool-artifact-retire", a, "first", "undeclared.txt", success=False)
    result = call(*retire_args, "--approve", preview["approval_sha256"])
    assert result["status"] == "succeeded" and result["payload_deleted"] is False
    assert call(*retire_args)["already_retired"] is True
    observed = call("spool-artifacts", a, "first")
    assert observed["outputs"] == products["outputs"]
    assert observed["admission"]["batch.txt"]["mode"] == "retired"
    assert observed["admission"]["spare.txt"]["mode"] == "active"
    spare_output = next(v for v in products["outputs"] if v["path"] == "spare.txt")
    call("artifact-checkout", catalog, spare_output["binding"]["revision"], root / "spare.txt")
    assert (root / "spare.txt").read_text() == "spare"
    assert all((folder / name).read_bytes() == value for name, value in original.items())
    call("artifact-checkout", catalog, revision, root / "refused.txt", success=False)
    call("artifact-edit", catalog, revision, saved, hashlib.sha256(saved.read_bytes()).hexdigest(), success=False)
    call(*waiting_args, "--approve", waiting["approval_sha256"], success=False)
    call("spool-start-data-state", c, "direct", selection, 30, "--approve", direct["approval_sha256"], success=False)
    call("spool-events-dispatch", ws, dispatch, "--approve", scheduled["approval_sha256"], success=False)
    assert not (c / "runs" / waiting_run / "job").exists()
    assert not (c / "runs/direct/job").exists() and not (ws / "dispatches/not-started").exists()
    assert not (ws / "deliveries" / waiting_run).exists()
    pending = call("spool-events-pending", ws)["deliveries"]
    blocked = next(v for v in pending if v["subscription"] == "waiting" and v["event"] == event)
    assert blocked["phase"] == "blocked" and "retired" in blocked["admission_refusal"]
    call("spool-event-publish", other, a, "first", "batch.txt", success=False)
    assert call("spool-event-publish", ws, a, "first", "batch.txt")["already_published"] is True
    assert finish(process)["status"] == "succeeded"
    approved("spool-accept", b, run)
    # A committed checkpoint is an independent copy, not a new use of its accepted source output.
    approved("spool-artifact-retire", b, run, "state.txt")
    head = approved("spool-state-commit", b, heads[b], run)["revision"]
    approved("spool-artifact-retire", b, run, "report.txt")
    approved("spool-event-complete", ws, event, "active")
    approved("spool-reclaim", b, run)
    approved("spool-reclaim-snapshots", b, run)
    assert call(*live_args)["disposition"] == "already-consumed"
    checkpoint = call("spool-state-show", b, head)["selected"]
    call("artifact-checkout", checkpoint["catalog"], head, root / "state.txt")
    assert (root / "state.txt").read_text() == "2\n"
    assert call("spool-state-show", c)["head"] == heads[c]
    # A fresh exact version remains selectable; retirement does not cancel admitted work.
    newer, next_event = produce("next", "3\n")
    cancelled, cancelled_run, cancel_args = start_live(c, next_event, "waiting")
    approved("spool-artifact-retire", a, "next", "batch.txt")
    cancelled.send_signal(signal.SIGINT)
    stopped = finish(cancelled, success=False)
    assert stopped["status"] == "cancelled" and stopped["result"]["workspace"]["termination_confirmed"] is True
    call("spool-accept", c, cancelled_run, success=False)
    approved("spool-reclaim", c, cancelled_run)
    approved("spool-reclaim-snapshots", c, cancelled_run)
    assert call("spool-state-show", c)["head"] == heads[c]
    assert call(*cancel_args, success=False)["disposition"] == "submitted-not-confirmed"
    assert len(list((c / "runs" / cancelled_run / "job/attempts").iterdir())) == 1
    assert saved.read_text() == "2\n"  # Existing independent copies are not revoked.
    # Simulate a torn closure record for another exact output, never overwrite or retry it.
    spare = next(v for v in newer["outputs"] if v["path"] == "spare.txt")
    spare_dir = Path(newer["catalog"]) / "revisions" / spare["binding"]["revision"]
    damaged = spare_dir / "admission-retired.json"
    damaged.write_bytes(b"{")
    call("spool-artifact-retire", a, "next", "spare.txt", success=False)
    call("artifact-checkout", newer["catalog"], spare["binding"]["revision"], root / "bad.txt", success=False)
    assert damaged.read_bytes() == b"{" and (spare_dir / "content.txt").read_text() == "spare"
    summary = {"schema": "native-artifact-retirement-qualification-v1", "passed": True,
        "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(), "running_consumer_completed": True,
        "new_consumers_refused": True, "unrelated_output_available": True, "checkpoint_commit_after_retirement": True,
        "cancellation_confirmed": True, "formal_payloads_preserved": True, "payloads_deleted": 0,
        "unknown_retirement_retried": False, "automatic_retries": 0, "model_invoked": False}
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()

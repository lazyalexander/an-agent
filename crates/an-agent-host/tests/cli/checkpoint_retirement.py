#!/usr/bin/env python3
"""Real native checkpoint closure, in-flight and uncertain readers, and receipted pin release."""
import argparse
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import time

from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    require_native_workspace()
    host, root = args.host.resolve(strict=True), args.evidence.resolve()
    root.mkdir(mode=0o700)

    def spawn(*argv):
        return subprocess.Popen(list(map(str, (host, *argv))), text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def finish(proc, ok=True, error=None):
        try:
            out, err = proc.communicate(timeout=35)
        except subprocess.TimeoutExpired as exc:
            proc.send_signal(signal.SIGINT)
            proc.communicate(timeout=10)
            raise RuntimeError("Client deadline exceeded; retain evidence, no replay") from exc
        with (root / "commands.local.jsonl").open("a") as log:
            log.write(json.dumps({"argv": proc.args, "code": proc.returncode, "out": out, "err": err}) + "\n")
        assert (proc.returncode == 0) == ok and (error is None or error in err), (proc.args, out, err)
        return json.loads(out) if out else None

    def call(*argv, **opts):
        return finish(spawn(*argv), **opts)

    def approve(*argv):
        return call(*argv, "--approve", call(*argv)["approval_sha256"])

    package = root / "release"
    (package / "silk").mkdir(parents=True)
    script = ('set -eu\nread -r mode < "$AN_SPOOL_INPUT"\n'
              'case "$mode" in gate) printf ready > ready.marker; while [ ! -f finish.marker ]; do sleep .05; done;; hold|lost) printf ready > ready.marker\n'
              ' if [ "$mode" = hold ]; then sleep 60; else sleep 2; fi;; esac\n'
              'cat "$AN_SPOOL_INPUT" > state.txt\nprintf shared > other.txt\n')
    (package / "silk/main.sh").write_text(script)
    (package / "native-spool.json").write_text(json.dumps({
        "schema": "an-native-spool-v3", "name": "retirement", "version": "1", "entry": "silk/main.sh",
        "files": {"silk/main.sh": hashlib.sha256(script.encode()).hexdigest()},
        "outputs": ["state.txt", "other.txt"], "events": {"consumes": [], "produces": ["artifact.accepted"]},
        "requirements": {"runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
                         "interaction": "headless", "gpu": "none", "inputs": ["input.txt"], "editable": [],
                         "dependencies": [], "capabilities": ["read-input", "write-declared-outputs"]}}))
    producer = root / "producer"
    call("spool-load", package, producer)
    seed = root / "seed.txt"

    def produce(run):
        seed.write_text(run + "\n")
        approve("spool-start", producer, run, seed, 15)
        approve("spool-accept", producer, run, "--registered")
        report = call("spool-artifacts", producer, run)
        return {p["path"]: {"instance": str(producer), "run": run, "output": p["path"],
                            "revision": p["binding"]["revision"]} for p in report["outputs"]}, Path(report["catalog"])

    def manage(op, p):
        return ["spool-managed", op, producer, p["run"], p["output"], p["revision"]]

    def checkpoint(name, value, pins, registered=True):
        instance = root / name
        call("spool-load", package, instance)
        seed.write_text(value + "\n")
        revision = approve("spool-state-init", instance, "state.txt", seed, *(["--registered"] if registered else []))["state"]["revision"]
        selection = root / (name + ".json")
        selection.write_text(json.dumps({"schema": "an-native-checkpoint-dependencies-v1", "artifacts": pins}))
        approve("spool-state-dependencies", instance, revision, selection)
        return instance, revision

    def running(command, instance, name):
        proc = spawn(*command, "--approve", call(*command)["approval_sha256"])
        deadline = time.monotonic() + 15
        while not (instance / "runs" / name / "job/work/ready.marker").is_file():
            if proc.poll() is not None:
                raise RuntimeError(proc.communicate())
            if time.monotonic() > deadline:
                proc.send_signal(signal.SIGINT)
                finish(proc, ok=False)
                raise RuntimeError("Readiness unconfirmed; no replay")
            time.sleep(.02)
        return proc

    products, _ = produce("first")
    private, shared = products["state.txt"], products["other.txt"]
    old, v = checkpoint("old", "hold", [private, shared])
    other, v2 = checkpoint("other", "quick", [shared])
    for p in (private, shared):
        approve(*manage("close", p))
    stale = ["spool-supervisor", "start-state", old, "unused", v, "10"]
    token = call(*stale)["approval_sha256"]
    proc = running(["spool-supervisor", "start-state", old, "holding", v, "90"], old, "holding")
    try:
        approve("spool-state-retire", old, v)
        assert call("spool-state-show", old, v)["admission"]["mode"] == "closed"
        call("spool-state-release", old, v, ok=False, error="release is unconfirmed")
        call(*manage("delete", private), ok=False, error="checkpoint")
    finally:
        approve("spool-supervisor", "cancel", old, "holding")
        cancelled = finish(proc, ok=False)
    assert cancelled["status"] == "cancelled" and cancelled["workspace"]["termination_confirmed"] is True
    call(*stale, "--approve", token, ok=False, error="admission is closed")
    assert not (old / "runs/unused/supervisor").exists()
    call("spool-start-state", old, "late", v, 10, ok=False, error="admission is closed")
    approve("spool-reclaim", old, "holding")
    approve("spool-reclaim-snapshots", old, "holding")
    approve("spool-input-release", old, "holding")
    release = ["spool-state-release", old, v]
    preview = call(*release)
    assert preview["plan"]["reader_evidence"]["registered_readers"] == 1
    assert set(preview["plan"]["verified_reservations"]) == {"holding"}
    marker = old / "runs/holding/input-released.json"
    backup = marker.with_suffix(".saved")
    marker.rename(backup)
    call(*release, "--approve", preview["approval_sha256"], ok=False)
    assert not (old / "checkpoints/dependencies" / v / "release-intent.json").exists()
    backup.rename(marker)
    terminal = old / "runs/holding/supervisor/completion.json"
    saved_terminal = terminal.with_suffix(".saved")
    terminal.rename(saved_terminal)
    call(*release, "--approve", preview["approval_sha256"], ok=False, error="no confirmed outcome")
    assert not (old / "checkpoints/dependencies" / v / "release-intent.json").exists()
    saved_terminal.rename(terminal)
    receipt = call(*release, "--approve", preview["approval_sha256"])["receipt"]
    assert receipt["approval_sha256"] == preview["approval_sha256"] and len(receipt["released_references"]) == 2
    assert receipt["payloads_deleted"] is False and call(*manage("show", private))["phase"] == "closed"
    call(*release, "--approve", preview["approval_sha256"], ok=False, error="never replay")
    # A source release marker cannot substitute for its checkpoint's final receipt.
    final = old / "checkpoints/dependencies" / v / "release-receipt.json"
    saved = final.with_suffix(".saved")
    final.rename(saved)
    call(*manage("delete", private), ok=False, error="unconfirmed")
    saved.rename(final)
    approve(*manage("delete", private))
    assert call("spool-state-show", old, v)["dependencies"]["receipt"] == receipt
    call(*manage("delete", shared), ok=False, error="checkpoint")
    assert call("spool-start-state", other, "preview", v2, 10)["plan"]["state_input"]["binding"]["revision"] == v2
    approve("spool-state-retire", other, v2)
    approve("spool-state-release", other, v2)
    assert call(*manage("show", shared))["phase"] == "closed"
    approve(*manage("delete", shared))

    # The process owns its input snapshot before the gate. The domain fixture
    # waits for an explicit test signal, so timing never decides which side wins.
    live_products, _ = produce("finish-gate")
    live, selected = checkpoint("finishes", "gate", [live_products["state.txt"]])
    worker = running(["spool-start-state", live, "finish", selected, 45], live, "finish")
    try:
        approve("spool-state-retire", live, selected)
        call("spool-state-release", live, selected, ok=False, error="release is unconfirmed")
    finally:
        (live / "runs/finish/job/work/finish.marker").touch(exist_ok=False)
    assert finish(worker)["workspace"]["termination_confirmed"] is True
    approve("spool-accept", live, "finish")
    successor = approve("spool-state-commit", live, selected, "finish")["revision"]
    view = call("spool-state-show", live, successor)
    assert view["admission"]["mode"] == "open" and view["selected"]["binding"]["parent"] == selected
    assert call("spool-state-show", live, selected)["admission"]["mode"] == "closed"
    assert call("spool-start-state", live, "next", successor, 15)["plan"]["state_input"]["binding"]["revision"] == successor
    call("spool-state-release", live, selected, ok=False, error="release is unconfirmed")

    second, _ = produce("unknown")
    held = second["state.txt"]
    lost, selected = checkpoint("lost-client", "lost", [held])
    approve(*manage("close", held))
    client = running(["spool-start-state", lost, "original", selected, 15], lost, "original")
    client.kill()  # Only this submitting test CLI. Its child has a finite workload.
    finish(client, ok=False)
    approve("spool-state-retire", lost, selected)
    call("spool-state-release", lost, selected, ok=False, error="release is unconfirmed")
    call(*manage("delete", held), ok=False, error="checkpoint")
    observed = call("spool-status", lost, "original")
    assert observed["termination_confirmed"] is False and len(list((lost / "runs/original/job/attempts").iterdir())) == 1
    assert (lost / "runs/original/job/work").is_dir()

    # Fault between two source writes, after publication of the owning intent.
    partial1, _ = produce("partial-one")
    partial2, second_catalog = produce("partial-two")
    sources = [partial1["state.txt"], partial2["state.txt"]]
    partial, rev = checkpoint("partial", "quick", sources)
    for p in sources:
        approve(*manage("close", p))
    approve("spool-state-retire", partial, rev)
    cmd = ["spool-state-release", partial, rev]
    grant = call(*cmd)["approval_sha256"]
    blocker = second_catalog / "host.lock"
    with blocker.open("x") as lock:
        lock.write("disposable contention fixture")
    try:
        call(*cmd, "--approve", grant, ok=False)
    finally:
        blocker.unlink()
    assert call("spool-state-show", partial, rev)["dependencies"]["status"] == "release-unconfirmed"
    call(*cmd, "--approve", grant, ok=False, error="never replay")
    for p in sources:
        call(*manage("delete", p), ok=False)
        assert call(*manage("show", p))["phase"] == "closed"

    legacy_product, _ = produce("legacy-pinned")
    legacy, rev = checkpoint("legacy", "quick", [legacy_product["state.txt"]], registered=False)
    approve("spool-start-state", legacy, "upgrade", rev, 15)
    approve("spool-accept", legacy, "upgrade")
    promote = ["spool-state-commit", legacy, rev, "upgrade"]
    plain_grant = call(*promote)["approval_sha256"]
    call(*promote, "--registered", "--approve", plain_grant, ok=False, error="approval mismatch")
    upgraded = approve(*promote, "--registered")["revision"]
    assert call("spool-state-show", legacy, upgraded)["selected"]["binding"]["reader_profile"] == "registered-copy-v1"
    approve("spool-state-retire", legacy, rev)
    call("spool-start-state", legacy, "refused", rev, 15, ok=False, error="retired")
    call("spool-state-release", legacy, rev, ok=False, error="retain its pins")
    assert call("spool-state-show", legacy, rev)["dependencies"]["status"] == "registered"
    result = {"passed": True, "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
              "released_pins": 2, "confirmed_cancel": True, "independent_checkpoint_protected": True,
              "settled_across_retirement": True, "unsettled_attempts": 1, "partial_release_retained": True, "legacy_pins_retained": True, "enrolled_successor": True,
              "automatic_retries": 0, "receipt": receipt["approval_sha256"]}
    (root / "acceptance.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


if __name__ == "__main__":
    main()

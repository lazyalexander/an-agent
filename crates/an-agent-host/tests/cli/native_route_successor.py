#!/usr/bin/env python3
"""Real receiver cutover without moving historical or uncertain deliveries."""
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
    parser.add_argument("--legacy-host", type=Path)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    require_native_workspace()
    host, root = args.host.resolve(strict=True), args.evidence.resolve()
    legacy = args.legacy_host.resolve(strict=True) if args.legacy_host else None
    root.mkdir(mode=0o700)
    calls = []

    def spawn(*argv):
        return subprocess.Popen([str(host), *map(str, argv)], text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def finish(process, success=True):
        try:
            out, err = process.communicate(timeout=35)
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

    def call(*argv, success=True):
        return finish(spawn(*argv), success)

    def approved(*argv):
        preview = call(*argv)
        return call(*argv, "--approve", preview["approval_sha256"])

    def package(name, state=False, delay=0):
        release = root / (name + "-release")
        (release / "silk").mkdir(parents=True)
        code = 'set -eu\n'
        if state:
            code += f'printf ready > ready.marker\nsleep {delay}\n'
            code += 'awk \'{s += $1} END {print s}\' "$AN_SPOOL_INPUT" "$AN_SPOOL_STATE" > state.txt\n'
            code += f'printf "{name}=" > report.txt\ncat state.txt >> report.txt\n'
        else:
            code += 'cat "$AN_SPOOL_INPUT" > report.txt\n'
        (release / "silk/main.sh").write_text(code)
        requirements = {"runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
                        "interaction": "headless", "gpu": "none", "inputs": ["batch.txt"], "editable": [],
                        "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []}
        if state:
            requirements["state_input"] = "checkpoint.txt"
        (release / "native-spool.json").write_text(json.dumps({
            "schema": "an-native-spool-v3", "name": name, "version": "0.1.0", "entry": "silk/main.sh",
            "files": {"silk/main.sh": hashlib.sha256(code.encode()).hexdigest()},
            "outputs": ["state.txt", "report.txt"] if state else ["report.txt"], "requirements": requirements,
            "events": {"consumes": ["artifact.accepted"] if state else [], "produces": ["artifact.accepted"]}}))
        return release

    a, b, c, ws = (root / name for name in ["source", "old-instance", "new-instance", "workspace"])
    call("spool-load", package("source"), a)
    call("spool-load", package("old", True, 8), b)
    call("spool-load", package("new", True), c)
    call("spool-events-init", ws)
    seed = root / "seed.txt"
    seed.write_text("10\n")
    initial = approved("spool-state-init", b, "state.txt", seed)["state"]["revision"]

    def produce(name, value):
        seed.write_text(value)
        approved("spool-start", a, name, seed, 15)
        approved("spool-accept", a, name)
        return approved("spool-event-publish", ws, a, name, "report.txt")["event"]["id"]

    old_event = produce("first", "2\n")
    pending_event = produce("pending", "3\n")
    config = root / "route.json"

    def configure(target, source=a):
        config.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(source),
            "output": "report.txt", "target": str(target), "result": "report.txt", "wall_seconds": 30}))

    configure(b)
    approved("spool-event-subscribe", ws, "old", config)
    old_args = ["spool-event-deliver", ws, old_event, "old", "--state", initial]
    preview = call(*old_args)
    old_run = preview["plan"]["context"]["delivery"]
    pending = call("spool-event-deliver", ws, pending_event, "old", "--state", initial)
    pending_run = pending["plan"]["context"]["delivery"]
    batch = root / "batch.json"
    batch.write_text(json.dumps({"schema": "an-native-event-dispatch-request-v1", "name": "old-batch",
        "entries": [{"event": pending_event, "subscription": "old", "state_revision": initial}]}))
    batch_preview = call("spool-events-dispatch", ws, batch)
    configure(c)
    replace_args = ["spool-event-replace", ws, "old", "next", config]
    if legacy:
        subprocess.run([str(legacy), "spool-events-show", str(ws)], capture_output=True, check=True)
    process = spawn(*old_args, "--approve", preview["approval_sha256"])
    try:
        deadline = time.monotonic() + 15
        while not (b / "runs" / old_run / "job/work/ready.marker").exists():
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("No real readiness; no retry")
            time.sleep(.02)
        before = call("spool-events-show", ws)
        change = call(*replace_args)
        assert change["executed"] is False and change["plan"]["running_execution_cancelled"] is False
        call(*replace_args, "--approve", "0" * 64, success=False)
        assert call("spool-events-show", ws) == before
        raced = produce("raced", "4\n")
        call(*replace_args, "--approve", change["approval_sha256"], success=False)
        configure(c, source=b)
        call(*replace_args, success=False)  # This slice cannot silently change producers.
        configure(c)
        switched = approved(*replace_args)
        assert switched["execution_granted"] is False and switched["historical_events_replayed"] is False
        assert process.poll() is None  # Replacement did not signal current work.
        observation = call("spool-events-show", ws)
        previous = next(r for r in observation["subscriptions"] if r["name"] == "old")
        assert previous["retired_by"]["name"] == "next" and previous["admission_refusal"]
        assert json.loads((ws / "native-events.json").read_text())["schema"] == "an-native-event-workspace-v3"
        if legacy:
            old_host = subprocess.run([str(legacy), "spool-events-show", str(ws)], capture_output=True, text=True)
            assert old_host.returncode != 0 and "unsupported native event workspace" in old_host.stderr
        call("spool-event-pause", ws, "old", success=False)
        call("spool-event-resume", ws, "old", success=False)
        call("spool-events-dispatch", ws, batch, "--approve", batch_preview["approval_sha256"], success=False)
        call("spool-event-deliver", ws, pending_event, "old", "--state", initial,
             "--approve", pending["approval_sha256"], success=False)
        assert not (b / "runs" / pending_run / "job").exists()
        assert not (ws / "dispatches/old-batch").exists()
        for past in [old_event, pending_event, raced]:
            call("spool-event-deliver", ws, past, "next", success=False)
        assert list((c / "runs").iterdir()) == []
        old_result = finish(process)
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGINT)
            finish(process, False)
    assert old_result["result"]["workspace"]["termination_confirmed"] is True
    facade = b / "runs" / old_run / "result.json"
    saved = facade.read_bytes()
    facade.unlink()
    assert call(*old_args, success=False)["automatic_retries"] == 0
    call("spool-event-deliver", ws, old_event, "next", success=False)
    facade.write_bytes(saved)
    assert len(list((b / "runs" / old_run / "job/attempts").iterdir())) == 1
    approved("spool-accept", b, old_run)
    old_head = approved("spool-state-commit", b, initial, old_run)["revision"]
    approved("spool-event-complete", ws, old_event, "old")  # Historical custody still works.
    selected = call("spool-state-show", b, old_head)["selected"]
    seed_state = root / "retained-state.txt"
    call("artifact-checkout", selected["catalog"], old_head, seed_state)
    assert seed_state.read_text() == "12\n"
    new_initial = approved("spool-state-init", c, "state.txt", seed_state)["state"]["revision"]
    fresh = produce("fresh", "5\n")
    new_args = ["spool-event-deliver", ws, fresh, "next", "--state", new_initial]
    delivered = approved(*new_args)
    new_run = delivered["context"]["delivery"]
    approved("spool-accept", c, new_run)
    new_head = approved("spool-state-commit", c, new_initial, new_run)["revision"]
    approved("spool-event-complete", ws, fresh, "next")
    assert call(*new_args)["disposition"] == "already-consumed"
    # Existing controls remain usable on v3 without a schema downgrade.
    approved("spool-event-pause", ws, "next")
    approved("spool-event-resume", ws, "next")
    assert json.loads((ws / "native-events.json").read_text())["schema"] == "an-native-event-workspace-v3"
    assert call("spool-state-show", b)["head"] == old_head
    assert call("spool-state-show", c)["head"] == new_head
    for instance, run, expected in [(b, old_run, "old=12\n"), (c, new_run, "new=17\n")]:
        approved("spool-reclaim", instance, run)
        approved("spool-reclaim-snapshots", instance, run)
        outputs = call("spool-artifacts", instance, run)
        report = next(p for p in outputs["outputs"] if p["path"] == "report.txt")
        saved_report = root / (instance.name + "-report.txt")
        call("artifact-checkout", outputs["catalog"], report["binding"]["revision"], saved_report)
        assert saved_report.read_text() == expected
        assert len(list((instance / "runs" / run / "job/attempts").iterdir())) == 1
    summary = {"schema": "native-route-successor-qualification-v1", "passed": True,
        "host_sha256": hashlib.sha256(host.read_bytes()).hexdigest(),
        "old_delivery_retried": False, "historical_events_replayed": False,
        "running_delivery_cancelled": False, "old_custody_preserved": True,
        "fixed_state_transfer_qualified": True, "automatic_retries": 0, "model_invoked": False,
        "legacy_host_refusal_checked": legacy is not None,
        "legacy_host_sha256": hashlib.sha256(legacy.read_bytes()).hexdigest() if legacy else None}
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()

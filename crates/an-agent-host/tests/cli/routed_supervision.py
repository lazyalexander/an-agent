#!/usr/bin/env python3
"""Real client, finite coordinator and attempt-owner loss across normal native CLI entrypoints."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
from host_qualification import require_native_workspace


def wait_for(check, description):
    deadline = time.monotonic() + 20
    while not check():
        if time.monotonic() >= deadline:
            raise RuntimeError(f"Observation expired: {description}; retain original work")
        time.sleep(.025)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    opt = parser.parse_args()
    require_native_workspace()
    host, root = opt.host.resolve(strict=True), opt.evidence.resolve()
    root.mkdir(mode=0o700)
    children = []

    def spawn(args):
        p = subprocess.Popen(list(map(str, [host, *args])), text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        children.append(p)
        return p

    def finish(p, ok=True):
        out, err = p.communicate(timeout=45)
        with (root / "commands.local.jsonl").open("a") as log:
            log.write(json.dumps({"args": p.args, "code": p.returncode, "stdout": out, "stderr": err}) + "\n")
        assert (p.returncode == 0) == ok, (p.args, out, err)
        return json.loads(out) if out else None

    def call(*args, ok=True):
        return finish(spawn(args), ok)

    def approved(*args):
        return call(*args, "--approve", call(*args)["approval_sha256"])

    def job(instance, run):
        return instance / "runs" / run

    def start(args, receiver, run, preview):
        client = spawn([*args, "--approve", preview["approval_sha256"]])
        marker = job(receiver, run) / "job/work/ready.marker"
        wait_for(lambda: marker.exists() or client.poll() is not None, "real workload ready")
        assert marker.exists() and client.poll() is None, client.args
        client.kill()
        finish(client, False)
        assert client.returncode == -signal.SIGKILL

    def terminal(instance, run, status):
        directory = job(instance, run)
        wait_for(lambda: (directory / "supervisor/completion.json").is_file()
                 and call("spool-status", instance, run)["termination_confirmed"], "verified original supervisor receipt")
        value = call("spool-status", instance, run)
        assert value["execution_status"] == status and value["termination_confirmed"] is True, value
        result = json.loads((directory / "result.json").read_text())
        assert result["workspace"]["reaped"] and result["workspace"]["group_observation"]["empty"]
        assert len(list((directory / "job/attempts").iterdir())) == 1
        return result

    ws = root / "workspace"
    call("spool-events-init", ws)
    def load(name, code):
        package = root / (name + "-release")
        (package / "silk").mkdir(parents=True)
        (package / "silk/main.sh").write_text(code)
        (package / "native-spool.json").write_text(json.dumps({"schema": "an-native-spool-v3", "name": name,
            "version": "1", "entry": "silk/main.sh", "files": {"silk/main.sh": hashlib.sha256(code.encode()).hexdigest()},
            "outputs": ["report.txt"], "events": {"consumes": ["artifact.accepted"], "produces": ["artifact.accepted"]},
            "requirements": {"runtime": "macos-native-process-v1", "os": "macos", "arch": "portable-source",
                "interaction": "headless", "gpu": "none", "inputs": ["input.txt"], "editable": [],
                "capabilities": ["read-input", "write-declared-outputs"], "dependencies": []}}))
        target = root / name
        call("spool-load", package, target)
        return target

    source = load("source", 'set -eu\nread -r mode < "$AN_SPOOL_INPUT"\n'
                  'if [ "$mode" = reply:gate ]; then printf ready > ready.marker; '
                  'while [ ! -f finish.marker ]; do sleep .05; done; fi\ncat "$AN_SPOOL_INPUT" > report.txt\n')
    program = ('set -eu\nread -r mode < "$AN_SPOOL_INPUT"\nprintf ready > ready.marker\n'
               'case "$mode" in gate) while [ ! -f finish.marker ]; do sleep .05; done;; '
               'lose) n=0; while [ ! -f exit.marker ] && [ "$n" -lt 200 ]; do sleep .05; n=$((n+1)); done;; forever) sleep 60;; esac\ncat "$AN_SPOOL_INPUT" > report.txt\n')
    data = root / "seed"
    events = {}
    for name in ["gate", "quick", "forever", "lose"]:
        data.write_text(name + "\n")
        approved("spool-start", source, name, data, 15)
        approved("spool-accept", source, name)
        events[name] = approved("spool-event-publish", ws, source, name, "report.txt")["event"]["id"]

    def route(name, seconds=30):
        target = load(name, program)
        config = root / (name + ".json")
        config.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(source),
            "output": "report.txt", "target": str(target), "result": "report.txt", "wall_seconds": seconds}))
        approved("spool-event-subscribe", ws, name, config)
        return target

    def request(name, kinds):
        path = root / (name + "-selection.json")
        path.write_text(json.dumps({"schema": "an-native-event-dispatch-request-v1", "name": name,
            "entries": [{"event": events[k], "subscription": name} for k in kinds]}))
        return path

    try:
        timed = route("timed", 2)
        argv = ["spool-event-deliver", ws, events["forever"], "timed"]
        preview = call(*argv)
        run = preview["plan"]["context"]["delivery"]
        start(argv, timed, run, preview)
        terminal(timed, run, "timed_out")
        call("spool-accept", timed, run, ok=False)
        assert call(*argv, ok=False)["executed"] is False

        deadline_target = route("deadline", 2)
        deadline_selection = request("deadline", ["forever", "quick"])
        deadline_args = ["spool-events-dispatch", ws, deadline_selection]
        preview = call(*deadline_args)
        one, two = [e["plan"]["context"]["delivery"] for e in preview["plan"]["entries"]]
        start(deadline_args, deadline_target, one, preview)
        terminal(deadline_target, one, "timed_out")
        wait_for(lambda: call("spool-supervisor", "dispatch", ws, "deadline", "show")["result"] is not None,
                 "verified finite timeout")
        timed_summary = call("spool-events-dispatch-show", ws, "deadline", ok=False)
        assert timed_summary["status"] == "timed_out" and timed_summary["attempted_entries"] == 1
        assert not (job(deadline_target,two) / "job").exists()

        # An approved list continues after its client disappears, even if the request file goes away.
        batch = route("batch")
        spec = request("batch", ["gate", "quick"])
        argv = ["spool-events-dispatch", ws, spec]
        preview = call(*argv)
        ids = [e["plan"]["context"]["delivery"] for e in preview["plan"]["entries"]]
        nested_grant = preview["plan"]["entries"][0]["approval_sha256"]
        call("spool-event-deliver", ws, events["gate"], "batch", "--approve", nested_grant, ok=False)
        assert not (job(batch, ids[0]) / "job").exists()
        start(argv, batch, ids[0], preview)
        spec.unlink()
        (job(batch, ids[0]) / "job/work/finish.marker").touch(exist_ok=False)
        for run in ids:
            terminal(batch, run, "succeeded")
        owner = ws / "dispatches/batch/supervisor"
        wait_for(lambda: (owner / "completion.json").exists(), "batch completion")
        assert call("spool-events-dispatch-show", ws, "batch")["attempted_entries"] == 2
        assert all(not (job(batch, run) / "accepted.json").exists() for run in ids)
        # Removing only the terminal supervision proof invalidates all positive projections.
        record = job(batch, ids[0]) / "supervisor/completion.json"
        backup = record.with_suffix(".saved")
        record.rename(backup)
        assert not call("spool-status", batch, ids[0])["termination_confirmed"]
        call("spool-accept", batch, ids[0], ok=False)
        call("spool-reclaim", batch, ids[0], ok=False)
        assert call("spool-events-dispatch-show", ws, "batch", ok=False)["status"] == "uncertain"
        assert call("spool-supervisor", "dispatch", ws, "batch", "show")["result"] is None
        call("spool-reconcile-run", batch, ids[0], ok=False)
        backup.rename(record)
        # Explicit reconstruction may only restore the exact result the independent owner retained.
        facade = job(batch, ids[0]) / "result.json"
        expected = json.loads(facade.read_text())
        facade.unlink()
        assert approved("spool-reconcile-run", batch, ids[0])["binding"] == expected
        approved("spool-accept", batch, ids[0])
        approved("spool-event-complete", ws, events["gate"], "batch")
        assert call("spool-event-deliver", ws, events["gate"], "batch")["disposition"] == "already-consumed"

        for name, session in [("drain", False), ("threaded", True)]:
            target = route(name)
            spec = request(name, ["gate", "quick"])
            if session:
                call("session-create", ws, name)
                argv = ["session-run", ws, name, "one", spec]
            else:
                argv = ["spool-events-dispatch", ws, spec]
            preview = call(*argv)
            plan = preview["plan"]["dispatch"]["plan"] if session else preview["plan"]
            first, next_run = [e["plan"]["context"]["delivery"] for e in plan["entries"]]
            start(argv, target, first, preview)
            if session:
                approved("session-control", ws, name, "cancelled")
                terminal(target, first, "cancelled")
                wait_for(lambda: (ws / "sessions" / name / "threads/one/supervisor/completion.json").is_file(), "thread completion")
                assert call("session-show", ws, name)["threads"][0]["settled"]
                approved("session-control", ws, name, "closed")
            else:
                approved("spool-supervisor", "dispatch", ws, name, "stop")
                (job(target, first) / "job/work/finish.marker").touch(exist_ok=False)
                terminal(target, first, "succeeded")
                wait_for(lambda: (ws / "dispatches" / name / "supervisor/completion.json").is_file(), "drained controller")
                assert call("spool-events-dispatch-show", ws, name)["reason"] == "stopped"
            assert not (job(target, next_run) / "job").exists()

        peer = load("peer", program.replace('cat "$AN_SPOOL_INPUT" > report.txt', 'echo "reply:$mode" > report.txt'))
        def exchange(name, mode):
            spec = root / (name + "-peer.json")
            spec.write_text(json.dumps({"schema":"an-native-peer-request-v1","source_run":mode,
                "source_output":"report.txt","target":str(peer),"reply_output":"report.txt","wall_seconds":30}))
            approved("spool-peer", "request", source, name, spec)
            argv = ["spool-peer", "run", source, name, "request"]
            preview = call(*argv)
            run = preview["plan"]["run"]
            start(argv, peer, run, preview)
            return argv, run

        argv, run = exchange("peer-cancel", "forever")
        approved("spool-supervisor", "cancel", peer, run)
        terminal(peer, run, "cancelled")
        assert call("spool-peer", "show", source, "peer-cancel")["stages"]["request"]["outcome"] == "cancelled"
        call(*argv, ok=False)
        _, run = exchange("peer-success", "gate")
        (job(peer, run) / "job/work/finish.marker").touch(exist_ok=False)
        terminal(peer, run, "succeeded")
        approved("spool-accept", peer, run)
        approved("spool-peer", "reply", source, "peer-success")
        argv = ["spool-peer", "run", source, "peer-success", "response"]
        preview = call(*argv)
        response = preview["plan"]["run"]
        start(argv, source, response, preview)
        (job(source, response) / "job/work/finish.marker").touch(exist_ok=False)
        terminal(source, response, "succeeded")
        assert (job(source, response) / "job/work/report.txt").read_text() == "reply:gate\n"
        assert call("spool-peer", "show", source, "peer-success")["phase"] == "completed"

        # Losing the controller cannot orphan its active attempt or admit the next one.
        cut = route("cut")
        spec = request("cut", ["forever", "quick"])
        argv = ["spool-events-dispatch", ws, spec]
        preview = call(*argv)
        first, next_run = [e["plan"]["context"]["delivery"] for e in preview["plan"]["entries"]]
        start(argv, cut, first, preview)
        owner = ws / "dispatches/cut/supervisor"
        os.kill(json.loads((owner / "owner.json").read_text())["pid"], signal.SIGKILL)
        approved("spool-supervisor", "dispatch", ws, "cut", "cancel")
        terminal(cut, first, "cancelled")
        assert call(*argv, ok=False)["automatic_retries"] == 0
        call("spool-supervisor", "coordinator-worker", ws, "cut", preview["approval_sha256"], ok=False)
        assert not (job(cut, next_run) / "job").exists() and not (owner / "completion.json").exists()

        broken = route("broken")
        argv = ["spool-event-deliver", ws, events["lose"], "broken"]
        preview = call(*argv)
        run = preview["plan"]["context"]["delivery"]
        start(argv, broken, run, preview)
        directory = job(broken, run)
        os.kill(json.loads((directory / "supervisor/owner.json").read_text())["pid"], signal.SIGKILL)
        (directory / "job/work/exit.marker").touch(exist_ok=False)
        assert not call("spool-status", broken, run)["termination_confirmed"]
        call("spool-accept", broken, run, ok=False)
        call("spool-reclaim", broken, run, ok=False)
        assert call(*argv, ok=False)["status"] == "uncertain"
        assert len(list((directory / "job/attempts").iterdir())) == 1
        assert (directory / "job/work").is_dir() and not (directory / "supervisor/completion.json").exists()
        evidence = {"passed":True, "host_sha256":hashlib.sha256(host.read_bytes()).hexdigest(),
                    "event_after_client_loss":"timed_out", "batch_timeout":"timed_out", "batch_continued":2, "batch_drained":1,
                    "thread_cancelled":True, "peer_request_cancelled":True, "peer_reply_delivered":True,
                    "coordinator_loss_next_attempts":0, "supervisor_loss_original_attempts":1, "automatic_retries":0}
        (root / "acceptance.json").write_text(json.dumps(evidence, indent=2))
        print(json.dumps(evidence))
    finally:
        for child in children:
            if child.poll() is None:
                child.send_signal(signal.SIGINT)
                child.communicate(timeout=20)


if __name__ == "__main__":
    main()

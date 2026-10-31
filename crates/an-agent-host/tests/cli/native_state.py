#!/usr/bin/env python3
"""Real native spool state and round-control acceptance against one built host."""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest
from host_qualification import native_workspace_available, NATIVE_WORKSPACE_REASON


PARSER = argparse.ArgumentParser(description=__doc__)
PARSER.add_argument("--host", required=True, type=Path)
PARSER.add_argument("--evidence", type=Path)
PARSER.add_argument("--legacy-host", type=Path, help="Optional pre-control host for format-floor qualification")
OPTIONS = None


@unittest.skipUnless(native_workspace_available(), NATIVE_WORKSPACE_REASON)
class NativeState(unittest.TestCase):
    def setUp(self):
        parent = OPTIONS.evidence
        if parent:
            parent.mkdir(parents=True, exist_ok=True)
        self.base = Path(tempfile.mkdtemp(prefix="native-state-", dir=parent)).resolve()
        self.host = OPTIONS.host.resolve(strict=True)
        self.calls = []
        print(f"Retained evidence: {self.base}", flush=True)

    def tearDown(self):
        # Never remove execution evidence, including when termination is unknown.
        (self.base / "calls.json").write_text(json.dumps(self.calls, indent=2))
        (self.base / "environment.json").write_text(json.dumps({
            "platform": platform.platform(), "machine": platform.machine(),
            "host_sha256": hashlib.sha256(self.host.read_bytes()).hexdigest(),
        }, indent=2))

    def finish(self, process, args, success=True):
        try:
            stdout, stderr = process.communicate(timeout=20)
        except subprocess.TimeoutExpired:
            process.send_signal(signal.SIGINT)
            try:
                stdout, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired as error:
                raise AssertionError(
                    f"Host {process.pid} termination unknown; evidence retained; no retry"
                ) from error
            self.fail(f"Command deadline exceeded: {args}; {stderr}")
        self.calls.append({"args": list(map(str, args)), "code": process.returncode,
                           "executable": process.args[0], "stdout": stdout, "stderr": stderr})
        self.assertEqual(process.returncode == 0, success, stderr or stdout)
        return json.loads(stdout) if stdout else None

    def call(self, *args, success=True):
        process = subprocess.Popen([str(self.host), *map(str, args)],
                                   text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return self.finish(process, args, success)

    def approved(self, *args):
        preview = self.call(*args)
        return self.call(*args, "--approve", preview["approval_sha256"])

    def package(self):
        package = self.base / "package"
        (package / "silk").mkdir(parents=True)
        (package / "kit").mkdir()
        files = {
            "silk/main.sh": 'set -eu\n/bin/bash "$AN_SPOOL_KIT_DIR/step.sh" "$AN_SPOOL_INPUT"\n',
            "kit/step.sh": 'set -eu\ntest ! -e internal.tmp\nvalue=$(cat "$1")\n'
                           'printf "%s\\n" "$((value + 1))" > state.txt\n'
                           'printf "count=%s\\n" "$((value + 1))" > report.txt\n'
                           'printf scratch > internal.tmp\n',
        }
        for name, content in files.items():
            (package / name).write_text(content)
        (package / "native-spool.json").write_text(json.dumps({
            "schema": "an-native-spool-v2", "name": "counter", "version": "1.0.0",
            "entry": "silk/main.sh", "files": {
                name: hashlib.sha256(content.encode()).hexdigest()
                for name, content in files.items()
            }, "outputs": ["state.txt", "report.txt"], "requirements": {
                "runtime": "macos-native-process-v1", "os": "macos",
                "arch": "portable-source", "interaction": "headless", "gpu": "none",
                "inputs": ["state"], "editable": ["kit/step.sh"],
                "capabilities": ["read-input", "write-declared-outputs"], "dependencies": [],
            },
        }))
        return package

    def current(self, root):
        return self.call("spool-state-show", root)

    def run_state(self, root, run, revision):
        result = self.approved("spool-start-state", root, run, revision, "10")
        self.assertEqual(result["status"], "succeeded")
        self.assertTrue(result["workspace"]["termination_confirmed"])
        return result["workspace"]["attempt"]

    def test_large_snapshot_collection_preserves_products_state_and_continuation(self):
        package = self.package()
        kit = package / "kit/step.sh"
        kit.write_text('set -eu\nwc -c < "$1" > report.txt\nprintf "0\\n" > state.txt\n')
        manifest = package / "native-spool.json"
        content = json.loads(manifest.read_text())
        content["files"]["kit/step.sh"] = hashlib.sha256(kit.read_bytes()).hexdigest()
        manifest.write_text(json.dumps(content))
        root = self.base / "instance"
        self.call("spool-load", package, root)
        seed = self.base / "seed.txt"
        seed.write_text("0\n")
        checkpoint = self.approved("spool-state-init", root, "state.txt", seed)["state"]["revision"]
        data = self.base / "large.txt"
        length = 16 * 1024 * 1024
        data.write_bytes(b"x" * length)
        result = self.approved("spool-start", root, "large", data, "30")
        attempt = result["workspace"]["attempt"]
        directory = root / "runs/large/job/attempts" / attempt
        inputs = directory / "snapshot/inputs"
        self.call("spool-reclaim-snapshots", root, "large", success=False)
        self.approved("spool-accept", root, "large")
        artifacts = self.call("spool-artifacts", root, "large")
        self.call("spool-reclaim-snapshots", root, "large", success=False)
        self.approved("spool-reclaim", root, "large")
        self.approved("spool-suspend", root, checkpoint)
        before = self.call("spool-storage", root)
        source = next(item for item in before["runs"] if item["run"] == "large")
        self.assertGreaterEqual(source["snapshot_logical_bytes"], length)
        self.assertTrue(source["snapshot_cleanup_candidate"])
        self.assertGreater(before["filesystem_available_bytes"], 0)
        unknown = inputs / "unexpected.txt"
        unknown.write_text("operator material")
        self.call("spool-reclaim-snapshots", root, "large", success=False)
        unknown.unlink()
        outside = self.base / "outside.txt"
        outside.write_text("keep")
        unknown.symlink_to(outside)
        self.call("spool-reclaim-snapshots", root, "large", success=False)
        unknown.unlink()
        self.assertEqual(outside.read_text(), "keep")
        selected = inputs / data.name
        selected.chmod(0o600)
        selected.write_text("changed")
        self.call("spool-reclaim-snapshots", root, "large", success=False)
        selected.write_bytes(data.read_bytes())
        selected.chmod(0o400)
        plan = self.call("spool-reclaim-snapshots", root, "large")
        self.call("spool-reclaim-snapshots", root, "large", "--approve", "0" * 64, success=False)
        self.assertTrue(inputs.exists())
        collected = self.call("spool-reclaim-snapshots", root, "large", "--approve", plan["approval_sha256"])
        self.assertGreaterEqual(collected["result"]["removed_logical_bytes"], length)
        self.assertFalse(inputs.exists())
        after = self.call("spool-storage", root)
        target = next(item for item in after["runs"] if item["run"] == "large")
        self.assertEqual(target["snapshot_logical_bytes"], 0)
        self.assertEqual(target["artifact_logical_bytes"], source["artifact_logical_bytes"])
        self.assertEqual(after["state_logical_bytes"], before["state_logical_bytes"])
        self.assertEqual(self.call("spool-artifacts", root, "large"), artifacts)
        self.assertEqual(self.call("spool-status", root, "large")["artifact_status"], "accepted")
        repeated = self.call("spool-reclaim-snapshots", root, "large")
        self.assertFalse(repeated["executed"])
        self.assertEqual(repeated["status"], "succeeded")
        receipt = directory / "snapshot-reclaimed.json"
        saved = receipt.read_bytes()
        receipt.unlink()  # Completed deletion with a lost receipt remains uncertain.
        uncertain = self.call("spool-reclaim-snapshots", root, "large", success=False)
        self.assertEqual(uncertain["status"], "uncertain")
        self.assertFalse(receipt.exists())
        receipt.write_bytes(saved)
        report = next(item for item in artifacts["outputs"] if item["path"] == "report.txt")
        handoff = self.base / "handoff.txt"
        self.call("artifact-checkout", artifacts["catalog"], report["binding"]["revision"], handoff)
        self.assertEqual(int(handoff.read_text()), length)
        peer = self.base / "peer"
        self.call("spool-load", package, peer)
        self.call("spool-reclaim-snapshots", peer, "large", success=False)
        self.call("spool-start", peer, "stale-path", selected, "10", success=False)
        self.approved("spool-start", peer, "fixed", handoff, "10")
        self.approved("spool-accept", peer, "fixed")
        self.approved("spool-resume", root)
        self.run_state(root, "continue", checkpoint)
        self.approved("spool-accept", root, "continue")
        self.approved("spool-state-commit", root, checkpoint, "continue")
        self.assertEqual(Path(self.current(root)["selected"]["path"]).read_text(), "0\n")

    def test_storage_headroom_denial_records_positive_no_spawn_evidence(self):
        script = self.base / "tool.sh"
        script.write_text('printf should-not-run > report.txt\n')
        profile = self.base / "profile.json"
        value = {"schema": "an-workspace-job-v2", "executable": "/bin/bash",
                 "args": ["{input:0}"], "environment": {}, "readonly": [],
                 "inputs": [str(script)], "outputs": ["report.txt"], "wall_seconds": 10,
                 "trusted_process_group": True, "minimum_free_bytes": (1 << 64) - 1}
        profile.write_text(json.dumps(value))
        job = self.base / "job"
        preview = self.call("workspace-run", profile, job)
        denied = self.call("workspace-run", profile, job, "--approve",
                           preview["approval_sha256"], success=False)
        self.assertEqual(denied["schema"], "an-workspace-not-started-v1")
        self.assertFalse(denied["process_started"])
        self.assertTrue(denied["termination_confirmed"])
        self.assertEqual(denied["automatic_retries"], 0)
        self.assertIn("insufficient storage headroom", json.dumps(denied["error"]))
        directory = job / "attempts" / denied["attempt"]
        self.assertFalse((directory / "launch.json").exists())
        self.assertFalse((directory / "snapshot/inputs").exists())
        self.assertFalse((job / "work/report.txt").exists())
        self.call("workspace-accept", job, denied["attempt"], success=False)
        value["minimum_free_bytes"] = 0
        profile.write_text(json.dumps(value))
        low = self.call("workspace-run", profile, self.base / "low")
        self.assertGreaterEqual(low["plan"]["minimum_free_bytes"], 128 * 1024 * 1024 + script.stat().st_size)

    def test_round_pause_checkpoint_suspension_and_explicit_resume(self):
        package = self.package()
        root = self.base / "instance"
        self.call("spool-load", package, root)
        seed = self.base / "seed.txt"
        seed.write_text("0\n")
        self.call("spool-suspend", root, "0" * 64, success=False)
        initial = self.approved("spool-state-init", root, "state.txt", seed)["state"]["revision"]
        self.run_state(root, "first", initial)
        self.call("spool-pause", root, success=False)  # No formal acceptance yet.
        self.approved("spool-accept", root, "first")
        first = self.approved("spool-state-commit", root, initial, "first")["revision"]
        stale_start = self.call("spool-start-state", root, "next", first, "10")
        ticket = self.call("spool-kit-proposal-request", root, "step.sh")
        revoked = self.call("spool-kit-proposal-request", root, "step.sh")
        response = self.base / "proposal.json"
        response.write_text(json.dumps({"schema": "an-weaver-proposal-v1",
            "request": ticket["request"]["request"], "request_sha256": ticket["request_sha256"],
            "kit": "kit/step.sh", "content": ticket["request"]["content"]}))
        pause = self.call("spool-pause", root)
        self.assertFalse((root / "round-control.json").exists())
        def legacy(*args, success):
            if OPTIONS.legacy_host:
                process = subprocess.Popen([str(OPTIONS.legacy_host.resolve(strict=True)), *map(str, args)],
                                           text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                self.finish(process, args, success=success)
        legacy("spool-show", root, success=True)
        self.call("spool-pause", root, "--approve", "0" * 64, success=False)
        paused = self.call("spool-pause", root, "--approve", pause["approval_sha256"])
        self.assertEqual(paused["round_control"]["mode"], "paused")
        self.assertEqual(paused["round_control"]["generation"], 1)
        self.assertEqual(json.loads((root / "instance.json").read_text())["schema"],
                         "an-native-spool-instance-v2")
        legacy("spool-show", root, success=False)
        legacy("spool-start-state", root, "next", first, "10", "--approve",
               stale_start["approval_sha256"], success=False)
        control_head = root / "round-control.json"
        saved_head = self.base / "control-head.saved"
        control_head.rename(saved_head)
        self.call("spool-show", root, success=False)
        self.call("spool-start-state", root, "next", first, "10", "--approve",
                  stale_start["approval_sha256"], success=False)
        self.assertFalse(control_head.exists())
        self.assertEqual(self.call("spool-status", root, "first")["artifact_status"], "accepted")
        saved_head.rename(control_head)
        self.call("spool-start-state", root, "blocked", first, "10", success=False)
        self.call("spool-start", root, "blocked-file", seed, "10", success=False)
        self.call("spool-kit-reset", root, "step.sh", success=False)
        self.call("spool-kit-proposal-apply", root, response, success=False)
        self.call("spool-state-commit", root, initial, "first", success=False)
        self.call("spool-kit-proposal-revoke", root, revoked["request"]["request"])
        self.assertEqual(self.call("spool-status", root, "first")["artifact_status"], "accepted")
        self.approved("spool-reclaim", root, "first")
        self.assertEqual(self.current(root)["head"], first)
        repeated = self.call("spool-pause", root, "--approve", pause["approval_sha256"])
        self.assertTrue(repeated["already_recorded"])
        self.assertEqual(repeated["round_control"]["generation"], 1)
        unload = self.call("spool-unload", root)
        # Host-side fault injection: changed working code cannot silently resume.
        kit = root / "kit/step.sh"
        original = kit.read_bytes()
        kit.write_text("exit 9\n")
        self.call("spool-resume", root, success=False)
        kit.write_bytes(original)
        resume = self.call("spool-resume", root)
        self.call("spool-resume", root, "--approve", "0" * 64, success=False)
        resumed = self.call("spool-resume", root, "--approve", resume["approval_sha256"])
        self.assertEqual(resumed["round_control"]["generation"], 2)
        legacy("spool-show", root, success=False)
        self.assertFalse((root / "runs/next/job").exists())  # Resume never launches.
        self.call("spool-start-state", root, "next", first, "10", "--approve",
                  stale_start["approval_sha256"], success=False)
        self.call("spool-kit-proposal-apply", root, response, success=False)
        self.call("spool-unload", root, "--approve", unload["approval_sha256"], success=False)
        self.run_state(root, "next", first)
        self.approved("spool-accept", root, "next")
        second = self.approved("spool-state-commit", root, first, "next")["revision"]
        self.assertEqual(Path(self.current(root)["selected"]["path"]).read_text(), "2\n")

        # A retained start without workspace evidence is uncertain, never idle.
        planned = self.call("spool-start-state", root, "interrupted", second, "10")
        marker = root / "runs/interrupted/start.json"
        marker.write_text(json.dumps(planned["plan"]))
        self.call("spool-pause", root, success=False)
        self.call("spool-suspend", root, second, success=False)
        self.call("spool-unload", root, success=False)
        marker.unlink()  # Remove only this test's known, unexecuted fault marker.

        hold = self.base / "hold.sh"
        hold.write_text('set -eu\nprintf ready > ready\nsleep 30\n')
        self.approved("spool-kit-update", root, "step.sh", hold)
        args = ["spool-start-state", root, "holding", second, "30"]
        preview = self.call(*args)
        worker = subprocess.Popen([str(self.host), *map(str, args), "--approve",
                                   preview["approval_sha256"]], text=True,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 8
            while not (root / "runs/holding/job/work/ready").exists():
                if worker.poll() is not None or time.monotonic() > deadline:
                    self.fail("Native control witness never became ready")
                time.sleep(0.02)
            self.call("spool-pause", root, success=False)
            self.call("spool-suspend", root, second, success=False)
            self.call("spool-reclaim", root, "holding", success=False)
        finally:
            if worker.poll() is None:
                worker.send_signal(signal.SIGTERM)
            cancelled = self.finish(worker, args, success=False)
        self.assertEqual(cancelled["status"], "cancelled")
        self.assertTrue(cancelled["workspace"]["termination_confirmed"])
        self.approved("spool-kit-reset", root, "step.sh")
        self.approved("spool-reclaim", root, "holding")
        self.call("spool-suspend", root, first, success=False)
        suspension = self.call("spool-suspend", root, second)
        generation = suspension["plan"]["generation"]
        staging = root / "round-control" / f'{generation}-{suspension["approval_sha256"]}.pending'
        staging.write_text("conflicting staging")
        self.call("spool-suspend", root, second, "--approve",
                  suspension["approval_sha256"], success=False)
        self.assertEqual(self.call("spool-show", root)["round_control"]["mode"], "active")
        staging.unlink()
        suspended = self.call("spool-suspend", root, second, "--approve", suspension["approval_sha256"])
        self.assertEqual(suspended["round_control"]["mode"], "suspended")
        self.assertEqual(suspended["round_control"]["checkpoint"]["binding"]["revision"], second)
        content = Path(suspended["round_control"]["checkpoint"]["path"])
        saved = content.read_bytes()
        content.write_text("changed checkpoint")
        self.call("spool-resume", root, success=False)
        content.write_bytes(saved)
        self.approved("spool-reclaim", root, "next")
        self.assertFalse((root / "runs/next/job/work").exists())
        self.call("spool-start-state", root, "blocked-suspended", second, "10", success=False)
        other = self.base / "other"
        self.call("spool-load", package, other)
        selected = self.approved("spool-state-init", other, "state.txt", seed)["state"]["revision"]
        self.run_state(other, "independent", selected)
        self.approved("spool-accept", other, "independent")
        self.assertEqual(self.call("spool-show", root)["round_control"]["mode"], "suspended")
        resumed = self.approved("spool-resume", root)
        self.assertEqual(resumed["round_control"]["generation"], 4)
        self.call("spool-suspend", root, second, "--approve",
                  suspension["approval_sha256"], success=False)
        self.run_state(root, "continued", second)
        self.approved("spool-accept", root, "continued")
        third = self.approved("spool-state-commit", root, second, "continued")["revision"]
        self.assertEqual(Path(self.current(root)["selected"]["path"]).read_text(), "3\n")
        self.approved("spool-pause", root)
        self.approved("spool-unload", root)
        self.call("spool-resume", root, success=False)
        self.assertEqual(self.current(root)["head"], third)

    def test_explicit_commit_conflict_recovery_and_fresh_environment(self):
        package = self.package()
        root = self.base / "instance"
        self.call("spool-load", package, root)
        seed = self.base / "seed.txt"
        seed.write_text("0\n")
        self.call("spool-state-init", root, "undeclared.txt", seed, success=False)
        preview = self.call("spool-state-init", root, "state.txt", seed)
        self.assertFalse((root / "checkpoints").exists())
        seed.write_text("9\n")
        self.call("spool-state-init", root, "state.txt", seed, "--approve",
                  preview["approval_sha256"], success=False)
        seed.write_text("0\n")
        first = self.call("spool-state-init", root, "state.txt", seed,
                          "--approve", preview["approval_sha256"])["state"]["revision"]
        self.call("spool-state-init", root, "state.txt", seed, success=False)
        self.assertEqual(self.current(root)["head"], first)
        # Identical bytes through the ordinary input route do not confer state authority.
        self.approved("spool-start", root, "ordinary", seed, "10")
        self.approved("spool-accept", root, "ordinary")
        self.call("spool-state-commit", root, first, "ordinary", success=False)
        content = Path(self.current(root)["selected"]["path"])
        plan = self.call("spool-start-state", root, "a", first, "10")
        content.write_text("999\n")
        self.call("spool-start-state", root, "a", first, "10", "--approve",
                  plan["approval_sha256"], success=False)
        self.assertFalse((root / "runs/a/job").exists())
        content.write_text("0\n")
        attempt_a = self.run_state(root, "a", first)
        attempt_b = self.run_state(root, "b", first)
        self.assertNotEqual(attempt_a, attempt_b)
        self.assertEqual(self.current(root)["head"], first)
        self.call("spool-state-commit", root, first, "a", success=False)
        self.approved("spool-accept", root, "a")
        self.approved("spool-accept", root, "b")
        stale = self.call("spool-state-commit", root, first, "b")
        commit = self.call("spool-state-commit", root, first, "a")
        self.call("spool-state-commit", root, first, "a", "--approve", "0" * 64, success=False)
        # A real filesystem failure after immutable publication but before HEAD rename.
        staging = root / "checkpoints" / f'HEAD-{commit["approval_sha256"]}.pending'
        staging.write_text("conflicting staging bytes")
        self.call("spool-state-commit", root, first, "a", "--approve",
                  commit["approval_sha256"], success=False)
        self.assertEqual(self.current(root)["head"], first)
        orphan = next(p.name for p in (root / "checkpoints/catalog/revisions").iterdir()
                      if p.name != first)
        self.call("spool-state-show", root, orphan, success=False)
        staging.unlink()
        # Mutating scratch after acceptance cannot replace the accepted state bytes.
        (root / "runs/a/job/work/state.txt").write_text("777\n")
        self.approved("spool-reclaim", root, "a")
        second = self.call("spool-state-commit", root, first, "a", "--approve",
                           commit["approval_sha256"])["revision"]
        self.assertEqual(Path(self.current(root)["selected"]["path"]).read_text(), "1\n")
        replay = self.call("spool-state-commit", root, first, "a", "--approve",
                           commit["approval_sha256"])
        self.assertTrue(replay["already_committed"])
        self.assertEqual(replay["revision"], second)
        conflict = self.call("spool-state-commit", root, first, "b", "--approve",
                             stale["approval_sha256"], success=False)
        self.assertEqual(conflict["status"], "conflict")
        self.assertEqual(conflict["automatic_retries"], 0)
        self.approved("spool-reclaim", root, "b")
        self.assertEqual(self.call("spool-state-show", root, first)["selected"]["binding"]["revision"], first)
        self.assertFalse((root / "runs/a/job/work").exists())
        self.run_state(root, "next", second)
        self.approved("spool-accept", root, "next")
        third = self.approved("spool-state-commit", root, second, "next")["revision"]
        self.assertEqual(Path(self.current(root)["selected"]["path"]).read_text(), "2\n")

        # A cancelled run with an apparent candidate must never advance state.
        edit = self.base / "hold.sh"
        edit.write_text('set -eu\nprintf "999\\n" > state.txt\nprintf ready > ready\nsleep 30\n')
        self.approved("spool-kit-update", root, "step.sh", edit)
        cancel_args = ["spool-start-state", root, "cancel", third, "10"]
        cancel_plan = self.call(*cancel_args)
        process = subprocess.Popen([str(self.host), *map(str, cancel_args), "--approve",
                                    cancel_plan["approval_sha256"]], text=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        deadline = time.monotonic() + 8
        while not (root / "runs/cancel/job/work/ready").exists():
            if process.poll() is not None or time.monotonic() > deadline:
                process.send_signal(signal.SIGINT) if process.poll() is None else None
                self.finish(process, cancel_args, success=False)
                self.fail("Cancellation witness never became ready")
            time.sleep(0.02)
        self.call("spool-state-commit", root, third, "cancel", success=False)
        process.send_signal(signal.SIGTERM)
        cancelled = self.finish(process, cancel_args, success=False)
        self.assertEqual(cancelled["status"], "cancelled")
        self.assertTrue(cancelled["workspace"]["termination_confirmed"])
        self.call("spool-state-commit", root, third, "cancel", success=False)
        self.call("spool-accept", root, "cancel", success=False)
        self.approved("spool-reclaim", root, "cancel")
        self.approved("spool-reclaim-snapshots", root, "cancel")
        self.assertEqual(self.current(root)["head"], third)

        # New instance, restored kit baseline, explicitly seeded from one fixed checkpoint.
        snapshot = self.current(root)["selected"]
        exported = self.base / "saved-state.txt"
        self.call("artifact-checkout", snapshot["catalog"], third, exported)
        self.approved("spool-unload", root)
        fresh = self.base / "fresh-instance"
        self.call("spool-load", package, fresh)
        self.call("spool-start-state", fresh, "foreign", third, "10", success=False)
        seeded = self.approved("spool-state-init", fresh, "state.txt", exported)["state"]["revision"]
        self.assertNotEqual(seeded, third)
        self.run_state(fresh, "continued", seeded)
        self.approved("spool-accept", fresh, "continued")
        self.approved("spool-state-commit", fresh, seeded, "continued")
        self.assertEqual(Path(self.current(fresh)["selected"]["path"]).read_text(), "3\n")
        self.assertEqual(self.current(root)["head"], third)
        self.assertEqual(len(list((root / "runs/a/job/attempts").iterdir())), 1)


if __name__ == "__main__":
    OPTIONS, remaining = PARSER.parse_known_args()
    unittest.main(argv=[__file__, *remaining])

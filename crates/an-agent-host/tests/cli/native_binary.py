#!/usr/bin/env python3
"""Real macOS Python runtime, binary events, cancellation and retained custody."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import signal
import subprocess
import sys
import time


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    options = parser.parse_args()
    if platform.system() != "Darwin":
        raise SystemExit("Real macOS execution required; no fallback")
    host, root = options.host.resolve(strict=True), options.evidence.resolve()
    root.mkdir(mode=0o700)
    calls = []

    def command(*args):
        return [str(host), *map(str, args)]

    def finish(process):
        try:
            out, err = process.communicate(timeout=30)
        except subprocess.TimeoutExpired as error:
            process.send_signal(signal.SIGINT)
            try:
                process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                raise RuntimeError(f"Termination unknown for {process.pid}; retain {root}") from error
            raise RuntimeError("Acceptance command exceeded deadline; no retry") from error
        calls.append({"args": process.args, "exit": process.returncode, "stdout": out, "stderr": err})
        (root / "commands.local.json").write_text(json.dumps(calls, indent=2))
        return out, err

    def spawn(*args):
        return subprocess.Popen(command(*args), text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def call(*args, refused=False):
        process = spawn(*args)
        out, err = finish(process)
        if refused:
            assert process.returncode != 0, (args, out)
            return err
        assert process.returncode == 0, (args, out, err)
        return json.loads(out)

    def approved(*args):
        preview = call(*args)
        return call(*args, "--approve", preview["approval_sha256"])

    site = root / "site"
    site.mkdir()
    module = site / "support.py"
    original = b"SUFFIX = bytes(range(256))\n"
    module.write_bytes(original)
    runtime = root / "runtime.local.json"
    exe = Path(sys.executable).resolve(strict=True)
    selection = {"schema": "an-native-python-runtime-v1", "executable": str(exe),
                 "executable_sha256": digest(exe.read_bytes()), "stdlib": str(Path(sys.base_prefix).resolve()),
                 "site": str(site), "site_sha256": digest(json.dumps({"support.py": digest(original)},
                     separators=(",", ":")).encode()), "dependencies": ["support-1"]}
    runtime.write_text(json.dumps(selection))
    source = root / "source"
    source.mkdir()
    (source / "silk").mkdir()
    entry = (b'import os, time\nfrom pathlib import Path\nimport support\n'
             b'try:\n    Path(support.__file__).with_name("escape.py").write_text("forbidden")\n'
             b'except PermissionError:\n    pass\nelse:\n    raise RuntimeError("runtime was writable")\n'
             b'data = Path(os.environ["AN_SPOOL_INPUT"]).read_bytes()\n'
             b'if data in [b"hold", b"short"]:\n'
             b'    Path("tmp/ready").write_text("ready")\n'
             b'    time.sleep(60 if data == b"hold" else 1.5)\n'
             b'Path("report.bin").write_bytes(data + support.SUFFIX)\n')
    (source / "silk/run.py").write_bytes(entry)
    manifest = {"schema": "an-native-spool-v3", "name": "binary-example", "version": "0.1.0",
                "entry": "silk/run.py", "files": {"silk/run.py": digest(entry)},
                "outputs": ["report.bin"], "binary_outputs": ["report.bin"],
                "events": {"consumes": ["artifact.accepted"], "produces": ["artifact.accepted"]},
                "requirements": {"runtime": "macos-native-python-v1", "os": "macos",
                    "arch": "portable-source", "interaction": "headless", "gpu": "none",
                    "inputs": ["source"], "editable": [], "dependencies": ["support-1"],
                    "capabilities": ["read-input", "write-declared-outputs"]}}
    (source / "native-spool.json").write_text(json.dumps(manifest))
    a, b = root / "a", root / "b"
    call("spool-load", source, a, refused=True)
    assert not a.exists()
    bad = root / "wrong-runtime.local.json"
    altered = dict(selection, dependencies=["support-2"])
    bad.write_text(json.dumps(altered))
    call("spool-load", source, a, "--runtime", bad, refused=True)
    assert not a.exists()
    for instance in [a, b]:
        loaded = call("spool-load", source, instance, "--runtime", runtime)
        assert loaded["runtime_sha256"] == digest(runtime.read_bytes())
        assert loaded["execution_granted"] is False
    seed = root / "seed.dat"
    seed.write_bytes(b"first\x00\xff")
    preview = call("spool-start", a, "first", seed, 15)
    module.write_bytes(b"SUFFIX = b'changed'\n")
    call("spool-start", a, "first", seed, 15, "--approve", preview["approval_sha256"], refused=True)
    assert not (a / "runs/first/job").exists()
    module.write_bytes(original)
    run = approved("spool-start", a, "first", seed, 15)
    assert run["status"] == "succeeded" and run["workspace"]["termination_confirmed"] is True
    accepted = approved("spool-accept", a, "first")
    old = accepted["result"]["outputs"][0]["binding"]["revision"]
    # Bind byte encoding as well as hashes. Change both facade copies so the
    # earlier facade-equality check cannot mask this annotation mismatch.
    job = a / "runs/first/job/attempts" / run["workspace"]["attempt"] / "accepted.json"
    facade = a / "runs/first/accepted.json"
    originals = {path: path.read_bytes() for path in [job, facade]}
    altered = json.loads(originals[job])
    altered["outputs"][0]["binding"]["encoding"] = "text"
    bound = json.loads(originals[facade])
    bound["result"] = altered
    try:
        job.write_text(json.dumps(altered))
        facade.write_text(json.dumps(bound))
        call("spool-artifacts", a, "first", refused=True)
    finally:
        for path, data in originals.items():
            path.write_bytes(data)
    expected = seed.read_bytes() + bytes(range(256))
    ws = root / "workspace"
    call("spool-events-init", ws)
    route = root / "route.local.json"
    route.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(a),
                                 "target": str(b), "output": "report.bin", "result": "report.bin",
                                 "wall_seconds": 15}))
    approved("spool-event-subscribe", ws, "binary", route)
    event = approved("spool-event-publish", ws, a, "first", "report.bin")["event"]
    assert event["event"]["artifact"]["binding"]["encoding"] == "bytes"
    seed.write_bytes(b"second")
    approved("spool-start", a, "second", seed, 15)
    approved("spool-accept", a, "second")
    delivered = approved("spool-event-deliver", ws, event["id"], "binary")
    assert delivered["status"] == "succeeded" and delivered["consumed"] is False
    downstream = delivered["context"]["delivery"]
    assert delivered["result"]["workspace"]["termination_confirmed"] is True
    call("spool-event-complete", ws, event["id"], "binary", refused=True)
    approved("spool-accept", b, downstream)
    complete = approved("spool-event-complete", ws, event["id"], "binary")
    assert complete["receipt"]["result_event"]["event"]["cause"] == delivered["context"]
    artifacts = call("spool-artifacts", b, downstream)
    revision = artifacts["outputs"][0]["binding"]["revision"]
    saved = root / "report.bin"
    call("artifact-checkout", artifacts["catalog"], revision, saved)
    assert saved.read_bytes() == expected + bytes(range(256))
    assert event["event"]["artifact"]["binding"]["revision"] == old

    def running(name, payload):
        seed.write_bytes(payload)
        preview = call("spool-start", a, name, seed, 30)
        process = spawn("spool-start", a, name, seed, 30, "--approve", preview["approval_sha256"])
        deadline = time.monotonic() + 10
        while not (a / "runs" / name / "job/work/tmp/ready").exists():
            if process.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError(f"Readiness unknown; retain evidence for {process.pid}")
            time.sleep(.02)
        return process

    short = running("runtime-change", b"short")
    module.write_bytes(b"SUFFIX = b'changed'\n")
    out, err = finish(short)
    assert short.returncode == 0 and json.loads(out)["status"] == "succeeded", err
    call("spool-accept", a, "runtime-change", refused=True)
    module.write_bytes(original)
    approved("spool-accept", a, "runtime-change")
    holding = running("cancel", b"hold")
    call("spool-reclaim", a, "cancel", refused=True)
    holding.send_signal(signal.SIGINT)
    out, err = finish(holding)
    cancelled = json.loads(out)
    assert holding.returncode != 0 and "execution did not succeed" in err, err
    assert cancelled["status"] == "cancelled" and cancelled["workspace"]["termination_confirmed"] is True
    assert not (site / "escape.py").exists()
    call("spool-accept", a, "cancel", refused=True)
    approved("spool-reclaim", a, "cancel")

    # Accepted products and scratch cleanup must not depend on retaining Python.
    site.rename(root / "site-unavailable")
    for instance, name in [(a, "first"), (a, "second"), (a, "runtime-change"), (b, downstream)]:
        approved("spool-reclaim", instance, name)
        approved("spool-reclaim-snapshots", instance, name)
    duplicate = call("spool-event-deliver", ws, event["id"], "binary")
    assert duplicate["executed"] is False and duplicate["disposition"] == "already-consumed"
    assert len(list((b / "runs" / downstream / "job/attempts").iterdir())) == 1
    retained = root / "retained.bin"
    call("artifact-checkout", artifacts["catalog"], revision, retained)
    assert retained.read_bytes() == saved.read_bytes()
    summary = {"schema": "native-binary-qualification-v1", "passed": True,
               "host_sha256": digest(host.read_bytes()), "source_revision": old,
               "result_revision": revision, "result_bytes": len(expected) + 256,
               "runtime_change_refused": True, "cancel_termination_confirmed": True,
               "encoding_tampering_refused": True,
               "retained_without_runtime": True, "automatic_retries": 0}
    (root / "acceptance.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()

"""The original native event identity survives explicit collection of its binary input."""
import json


def qualify_delivery(root, package, producer, source, call, approve, produce):
    run = "binary-event"
    report = produce(run)
    output = "bulk.txt"
    revision = next(o["binding"]["revision"] for o in report["outputs"] if o["path"] == output)
    sink, ws = root / "binary-consumer", root / "events"
    call("spool-load", package, sink)
    call("spool-events-init", ws)
    route = root / "binary-route.json"
    route.write_text(json.dumps({"schema": "an-native-artifact-route-v1", "source": str(producer),
        "target": str(sink), "output": output, "result": "state.txt", "wall_seconds": 15}))
    for name in ("current", "late"):
        approve("spool-event-subscribe", ws, name, route)
    event = approve("spool-event-publish", ws, producer, run, output)["event"]
    assert event["event"]["artifact"]["binding"]["revision"] == revision
    deliver = ["spool-event-deliver", ws, event["id"]]
    late = call(*deliver, "late")
    current = call(*deliver, "current")
    job = current["plan"]["context"]["delivery"]
    profile = sink / "runs" / job / "profile.json"
    call("workspace-run", profile, root / "unmanaged", fail="requires a registered reader")
    assert not (root / "unmanaged").exists()
    manage = lambda op: ["spool-managed", op, producer, run, output, revision]
    assert call(*manage("show"))["readers"] == {}
    done = call(*deliver, "current", "--approve", current["approval_sha256"])
    assert done["result"]["workspace"]["termination_confirmed"] is True
    approve("spool-accept", sink, job)
    result = call("spool-artifacts", sink, job, "--output", output)
    copy = root / "binary-result.bin"
    call("artifact-checkout", result["catalog"], result["output"]["binding"]["revision"], copy)
    assert copy.read_bytes() == source.read_bytes()
    approve(*manage("close"))
    call(*deliver, "late", "--approve", late["approval_sha256"], fail="admission is closed")
    assert not (sink / "runs" / late["plan"]["context"]["delivery"] / "job").exists()
    approve("spool-reclaim", sink, job)
    approve("spool-reclaim-snapshots", sink, job)
    release = ["spool-input-release", sink, job, "--delivery", ws, "current"]
    call(*release, fail="original consumption acknowledgement")
    call(*manage("delete"), fail="release is unconfirmed")
    approve("spool-event-complete", ws, event["id"], "current")
    approve(*release)
    receipt = approve(*manage("delete"))["receipt"]
    assert receipt["logical_bytes"] == source.stat().st_size
    assert call(*deliver, "current")["disposition"] == "already-consumed"
    # Once confirmed, source evidence can verify deletion without the consumer filesystem.
    sink.rename(root / "offline-consumer")
    assert call(*manage("show"))["receipt"] == receipt
    return revision

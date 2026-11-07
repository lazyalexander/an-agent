"""One receiving release, two real card sessions and a retired model boundary."""
import hashlib
import json
from pathlib import Path
from uuid import UUID

READER, OBSERVER = [str(UUID(int=n)) for n in (1, 2)]


def card(agent):
    return {"v": 1, "id": agent, "model": {"base_url": "https://model.invalid/v1", "model": "unused"},
            "prompt": "Local review", "tools": [], "topology": {"type": "leaf"}, "kernel": "0.1.0"}


def qualify(call, approve, offer, sender, target, packet, root, upstream):
    def route(name, agent):
        return ["spool-peer", "deliver-agent", sender, name, agent]

    def tape(receipt):
        return (Path(receipt["session"]) / "memory.jsonl").read_bytes()

    private = "observation reserved for the other agent"
    message = dict(packet, text=private, sha256=hashlib.sha256(private.encode()).hexdigest())
    approve(*offer("independent-note", message))
    sibling = route("independent-note", OBSERVER)
    call(*sibling, success=False)  # A published offer has not reached the receiver.
    approve("spool-peer", "run", sender, "independent-note", "request")
    other = approve(*sibling)
    before = tape(other)
    assert private.encode() in before and packet["text"].encode() not in before

    selected = route("selected", READER)
    preview = call(*selected)
    grant = preview["approval_sha256"]
    assert preview["plan"]["agent"] == READER and preview["plan"]["model_disclosure"] is False
    assert packet["text"] not in json.dumps(preview)
    call(*route("selected", str(UUID(int=3))), success=False)
    call(*route("selected", OBSERVER), "--approve", grant, success=False)
    call(*selected, "--approve", upstream, success=False)
    destination = target / "agent-handoffs" / preview["plan"]["request"]
    assert not destination.exists()
    completion = sender / "peers/selected/request-result.json"
    held = completion.with_suffix(".held")
    completion.rename(held)
    try:
        call(*selected, "--approve", grant, success=False)
        assert not destination.exists()
    finally:
        held.rename(completion)
    receipt = call(*selected, "--approve", grant)
    saved = json.loads(Path(receipt["packet"]).read_bytes())
    assert saved == dict(packet, audience=READER, source="peer:" + grant)
    assert hashlib.sha256(Path(receipt["packet"]).read_bytes()).hexdigest() == receipt["packet_sha256"]
    selected_clips = [e for e in map(json.loads, tape(receipt).splitlines()) if e["id"] == receipt["clip"]]
    assert len(selected_clips) == 1 and json.loads(selected_clips[0]["content"]) == saved
    assert "external-context-untrusted" in selected_clips[0]["tags"]
    assert private.encode() not in tape(receipt) and tape(other) == before

    # A handoff grant cannot revive the retired model service or create a run.
    model_output = root / "fixture-model-result"
    call("model", root / "missing-profile.json", receipt["packet"], model_output,
         "--approve", grant, success=False)
    assert not model_output.exists() and tape(other) == before
    # A missing completion receipt keeps the durable admission closed.
    (destination / "receipt.json").unlink()
    call(*selected, "--approve", grant, success=False)
    assert tape(other) == before
    return {"selected_agent": READER, "sibling_unchanged": True, "model_service_retired": True}

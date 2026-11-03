"""One receiving release, two real card sessions and an observed model boundary."""
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
from threading import Thread
from uuid import UUID

READER, OBSERVER = [str(UUID(int=n)) for n in (1, 2)]


def card(agent):
    return {"v": 1, "id": agent, "model": {"base_url": "https://model.invalid/v1", "model": "unused"},
            "prompt": "Local review", "tools": [], "topology": {"type": "leaf"}, "kernel": "0.1.0"}


def qualify(call, approve, offer, sender, target, packet, root, upstream):
    observed = []

    class Endpoint(BaseHTTPRequestHandler):
        def do_POST(self):
            observed.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            body = json.dumps({"choices": [{"message": {"role": "assistant", "content": "received"}}],
                               "usage": {"prompt_tokens": 4, "completion_tokens": 1}}).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    def route(name, agent):
        return ["spool-peer", "deliver-agent", sender, name, agent]

    def tape(receipt):
        return (Path(receipt["session"]) / "memory.jsonl").read_bytes()

    with HTTPServer(("127.0.0.1", 0), Endpoint) as server:
        worker = Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            profile = root / "fixture-profile.json"
            profile.write_text(json.dumps({"schema": "an-model-profile-v1", "base_url":
                f"http://127.0.0.1:{server.server_port}/v1", "model": "loopback", "system_prompt":
                "Inspect only the selected data.", "max_tokens": 16}))
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

            model = ["model", profile, receipt["packet"], root / "fixture-model-result"]
            disclosure = call(*model)["approval_sha256"]
            call(*model, "--approve", grant, success=False)
            call("model", profile, other["packet"], model[-1], "--approve", disclosure, success=False)
            assert not observed and not model[-1].exists()
            assert call(*model, "--approve", disclosure)["status"] == "succeeded"
            call(*model, "--approve", disclosure, success=False)
            assert len(observed) == 1
            messages = observed[0]["messages"]
            assert any(m["role"] == "user" and packet["text"] in m["content"] for m in messages)
            assert all(packet["text"] not in m["content"] for m in messages if m["role"] != "user")
            assert private not in json.dumps(observed) and tape(other) == before
            # A missing completion receipt keeps the durable admission closed.
            (destination / "receipt.json").unlink()
            call(*selected, "--approve", grant, success=False)
            assert tape(other) == before
            return {"selected_agent": READER, "sibling_unchanged": True, "loopback_requests": len(observed)}
        finally:
            server.shutdown()
            worker.join()

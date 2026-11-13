#!/usr/bin/env python3
"""TaskSession to Cocoon wire fixture. This does not qualify VM execution."""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import struct
import sys
import threading
import time
import uuid
from app_server_support import Driver, sha
from host_qualification import require_native_workspace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    require_native_workspace()
    driver = Driver(args.host, args.evidence)
    driver.expires = int(time.time() * 1000) + 300000
    server = socket.socket(socket.AF_UNIX)
    try:
        endpoint = driver.root / 'cc.sock'
        server.bind(str(endpoint))
        server.listen()
        server.settimeout(30)
        vm = str(uuid.uuid4())
        operation = {'operation': 'context', 'vm': vm}
        profile_path = driver.root / 'cocoon.json'
        profile_path.write_text(json.dumps({'socket': str(endpoint), 'grant': 'a'*64,
            'caller': 'main', 'vm': vm, 'generation': 1}))
        profile_path.chmod(0o600)
        command = str(uuid.uuid4())
        request = {'op': 'cocoon-call', 'profile': str(profile_path),
            'profile_sha256': sha(profile_path), 'operation_id': command, 'operation': operation}
        fixture = driver.root / 'fixture'
        fixture.write_text('#!' + sys.executable + '\n' + '''import sys,json
if '--version' in sys.argv: print('codex-cli 0.166.0');sys.exit()
def send(v): print(json.dumps(v),flush=True)
for raw in sys.stdin:
 v=json.loads(raw)
 if v.get('method')=='initialize': send({'id':0,'result':{}})
 elif v.get('method')=='thread/start': send({'id':1,'result':{'thread':{'id':'main'}}})
 elif v.get('method')=='turn/start':
  send({'id':2,'result':{'turn':{'id':'turn'}}})
  send({'method':'turn/started','params':{'threadId':'main','turn':{'id':'turn'}}})
  send({'id':99,'method':'item/tool/call','params':{'threadId':'main','turnId':'turn','tool':'host_request',
   'namespace':None,'callId':'cocoon','arguments':{'request':v['params']['input'][0]['text']}}})
 elif v.get('id')==99:
  send({'method':'item/completed','params':{'threadId':'main','turnId':'turn','item':{
   'type':'agentMessage','phase':'final_answer','text':json.dumps({'ok':v['result']['success']})}}})
  send({'method':'turn/completed','params':{'threadId':'main','turn':{'id':'turn','status':'completed'}}})
''')
        fixture.chmod(0o700)
        binary = bytes(range(256)) * 40
        source = driver.root / 'source.pdf'
        source.write_bytes(binary)
        attempt = str(uuid.uuid4())
        common = {'path': '/workspace/document.bin', 'attempt': attempt,
            'sha256': sha(source), 'bytes': len(binary)}
        upload = dict(common, direction='upload', source=str(source))
        download = dict(common, direction='download')
        transfers = [dict(request, op='cocoon-transfer', operation_id=str(uuid.uuid4()), operation=plan)
            for plan in (upload, download)]
        fixture_text = fixture.read_text().replace("def send(v):", "pending=[]; success=True\ndef send(v):")
        fixture_text = fixture_text.replace("  send({'id':99,'method':'item/tool/call'", "  pending=json.loads(v['params']['input'][0]['text'])\n  selected=pending.pop(0)\n  send({'id':99,'method':'item/tool/call'", 1)
        fixture_text = fixture_text.replace("v['params']['input'][0]['text']}}})", "json.dumps(selected)}}})")
        fixture_text = fixture_text.replace(" elif v.get('id')==99:\n", " elif v.get('id')==99:\n  success=success and v['result']['success']\n  if pending:\n   send({'id':99,'method':'item/tool/call','params':{'threadId':'main','turnId':'turn','tool':'host_request','namespace':None,'callId':str(len(pending)),'arguments':{'request':json.dumps(pending.pop(0))}}})\n   continue\n")
        fixture_text = fixture_text.replace("{'ok':v['result']['success']}", "{'ok':success}")
        fixture.write_text(fixture_text)
        task = driver.task(fixture, json.dumps([request, *transfers]))
        profile = driver.profile(task)
        digest = hashlib.sha256(json.dumps(operation, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        rights = [{'kind': 'cocoon', 'profile_sha256': sha(profile_path), 'operation_sha256': digest}]
        for plan in (upload, download):
            rights.append({'kind': 'cocoon', 'profile_sha256': sha(profile_path),
                'operation_sha256': hashlib.sha256(json.dumps(plan, sort_keys=True, separators=(',', ':')).encode()).hexdigest()})
        rights.append({'kind': 'read-input', 'path': str(source), 'sha256': sha(source)})
        parent = driver.grant([profile, *rights], delegate=True)
        leaf = driver.grant([profile], subject='restricted', parent=parent)
        received = []
        errors = []
        def receive_exact(stream, size):
            result = b''
            while len(result) < size:
                chunk = stream.recv(size-len(result))
                if not chunk:
                    raise EOFError('incomplete frame')
                result += chunk
            return result
        def respond():
            try:
                for index in range(5):
                    stream, _ = server.accept()
                    with stream:
                        stream.settimeout(10)
                        size, = struct.unpack('!I', receive_exact(stream, 4))
                        assert 0 < size <= 32768
                        body = json.loads(receive_exact(stream, size))
                        assert body['credential'] == ''
                        received.append(body['request'])
                        if index == 0:
                            assert body['operation'] == operation
                            result = {'vm': vm, 'state': 'running'}
                        else:
                            guest = body['operation']['request']
                            op = guest['operation']
                            offset = op['offset']
                            chunk = binary[offset:offset+8192]
                            assert guest['identity']['attempt'] == attempt
                            if op['kind'] == 'write_bytes':
                                assert bytes.fromhex(op['content_hex']) == chunk
                                result = {'kind': 'written'}
                            else:
                                assert op['kind'] == 'read_chunk'
                                result = {'kind': 'chunk', 'content_hex': chunk.hex(),
                                    'next_offset': offset+len(chunk), 'eof': offset+len(chunk) == len(binary)}
                            result = {'identity': guest['identity'], 'result': {'Ok': result}}
                        response = json.dumps({'request': body['request'], 'caller': 'main',
                            'result': {'Ok': result}}).encode()
                        stream.sendall(struct.pack('!I', len(response)) + response)
            except Exception as error:
                errors.append(str(error))
        thread = threading.Thread(target=respond, daemon=True)
        thread.start()
        job = driver.approve({'op': 'harness', 'task': task}, parent)['id']
        driver.wait(job, parent)
        thread.join(35)
        assert not thread.is_alive() and not errors, errors
        assert len(received) == 5 and len(set(received)) == 5 and received[0] != command
        candidate = driver.service / 'jobs' / job / 'cocoon-transfers' / transfers[1]['operation_id'] / 'candidate.bin'
        assert candidate.read_bytes() == binary
        assert driver.request({'op': 'harness-result', 'id': job}, parent)['proposal'] == {'ok': True}
        status = driver.request({'op': 'cocoon-status', 'id': job, 'operation_id': command}, parent)
        assert status['state'] == 'observed' and not status['artifact_accepted']
        connected = driver.request({'op': 'harness-connect', 'id': job}, parent)
        assert connected['cocoon_operations'] == [status]
        driver.request({'op': 'cocoon-status', 'id': job, 'operation_id': command}, leaf, ok=False)
        refused = driver.approve({'op': 'harness', 'task': task}, leaf)['id']
        driver.wait(refused, leaf)
        assert driver.request({'op': 'harness-result', 'id': refused}, leaf)['proposal'] == {'ok': False}
        (driver.root / 'report.json').write_text(json.dumps({'passed': True, 'fixture_only': True,
            'model_calls': 0, 'vm_execution_tested': False, 'context_and_four_binary_chunks': True, 'single_thread_tool_sequence': True,
            'scoped_tool_dispatch': True, 'reconnected_receipt': True}, indent=2))
    finally:
        server.close()
        driver.close()


if __name__ == '__main__':
    main()

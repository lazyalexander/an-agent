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
        task = driver.task(fixture, json.dumps(request))
        profile = driver.profile(task)
        digest = hashlib.sha256(json.dumps(operation, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        parent = driver.grant([profile, {'kind': 'cocoon', 'profile_sha256': sha(profile_path),
            'operation_sha256': digest}], delegate=True)
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
                stream, _ = server.accept()
                with stream:
                    stream.settimeout(10)
                    size, = struct.unpack('!I', receive_exact(stream, 4))
                    assert 0 < size <= 32768
                    body = json.loads(receive_exact(stream, size))
                    assert body['operation'] == operation and body['credential'] == ''
                    received.append(body['request'])
                    response = json.dumps({'request': body['request'], 'caller': 'main',
                        'result': {'Ok': {'vm': vm, 'state': 'running'}}}).encode()
                    stream.sendall(struct.pack('!I', len(response)) + response)
            except Exception as error:
                errors.append(str(error))
        thread = threading.Thread(target=respond, daemon=True)
        thread.start()
        job = driver.approve({'op': 'harness', 'task': task}, parent)['id']
        driver.wait(job, parent)
        thread.join(35)
        assert not thread.is_alive() and not errors, errors
        assert len(received) == 1 and received[0] != command
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
            'model_calls': 0, 'vm_execution_tested': False, 'single_backend_request': True,
            'scoped_tool_dispatch': True, 'reconnected_receipt': True}, indent=2))
    finally:
        server.close()
        driver.close()


if __name__ == '__main__':
    main()

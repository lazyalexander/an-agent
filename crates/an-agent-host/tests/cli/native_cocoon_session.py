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
    parser.add_argument('--cancel-validation', action='store_true')
    parser.add_argument('--cocoon-profile', type=Path)
    parser.add_argument('--cocoon-attempt')
    args = parser.parse_args()
    if bool(args.cocoon_profile) != bool(args.cocoon_attempt):
        parser.error('--cocoon-profile and --cocoon-attempt must be selected together')
    live = json.loads(args.cocoon_profile.read_text()) if args.cocoon_profile else None
    require_native_workspace()
    driver = Driver(args.host, args.evidence)
    driver.expires = int(time.time() * 1000) + 300000
    server = socket.socket(socket.AF_UNIX)
    try:
        endpoint = Path(live['socket']) if live else driver.root / 'cc.sock'
        if not live:
            server.bind(str(endpoint))
            server.listen()
            server.settimeout(30)
        vm = live['vm'] if live else str(uuid.uuid4())
        operation = {'operation': 'context', 'vm': vm}
        profile_path = driver.root / 'cocoon.json'
        profile_path.write_text(json.dumps(live or {'socket': str(endpoint), 'grant': 'a'*64,
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
        attempt = args.cocoon_attempt or str(uuid.uuid4())
        common = {'path': '/workspace/document.bin', 'attempt': attempt,
            'sha256': sha(source), 'bytes': len(binary)}
        upload = dict(common, direction='upload', source=str(source))
        download = dict(common, direction='download')
        transfers = [dict(request, op='cocoon-transfer', operation_id=str(uuid.uuid4()), operation=plan)
            for plan in (upload, download)]
        package = driver.root / 'validator-package'
        (package / 'silk').mkdir(parents=True)
        program = 'set -eu\ncat "$AN_SPOOL_INPUT" > artifact.bin\n'
        if args.cancel_validation:
            program = 'set -eu\nprintf ready > validation-started\nsleep 30\ncat "$AN_SPOOL_INPUT" > artifact.bin\n'
        (package / 'silk/main.sh').write_text(program)
        (package / 'native-spool.json').write_text(json.dumps({
            'schema': 'an-native-spool-v3', 'name': 'cocoon-candidate-validator', 'version': '0.1.0',
            'events': {'consumes': [], 'produces': ['artifact.accepted']},
            'entry': 'silk/main.sh', 'files': {'silk/main.sh': hashlib.sha256(program.encode()).hexdigest()},
            'outputs': ['artifact.bin'], 'binary_outputs': ['artifact.bin'],
            'requirements': {'runtime': 'macos-native-process-v1', 'os': 'macos',
                'arch': 'portable-source', 'interaction': 'headless', 'gpu': 'none',
                'inputs': ['input'], 'editable': [], 'dependencies': [],
                'capabilities': ['read-input', 'write-declared-outputs']}}))
        instance = driver.root / 'validator-instance'
        driver.call('spool-load', package, instance)
        driver.request({'op': 'register', 'instance': str(instance)})
        fixture_text = fixture.read_text().replace("def send(v):", "pending=[]; success=True\ndef send(v):")
        fixture_text = fixture_text.replace("  send({'id':99,'method':'item/tool/call'", "  pending=json.loads(v['params']['input'][0]['text'])\n  selected=pending.pop(0)\n  send({'id':99,'method':'item/tool/call'", 1)
        fixture_text = fixture_text.replace("v['params']['input'][0]['text']}}})", "json.dumps(selected)}}})")
        fixture_text = fixture_text.replace(" elif v.get('id')==99:\n", " elif v.get('id')==99:\n  success=success and v['result']['success']\n  if pending:\n   send({'id':99,'method':'item/tool/call','params':{'threadId':'main','turnId':'turn','tool':'host_request','namespace':None,'callId':str(len(pending)),'arguments':{'request':json.dumps(pending.pop(0))}}})\n   continue\n")
        fixture_text = fixture_text.replace("{'ok':v['result']['success']}", "{'ok':success}")
        fixture.write_text(fixture_text)
        fixture_text = fixture_text.replace("pending=[]; success=True", "pending=[]; success=True; turn_count=0")
        fixture_text = fixture_text.replace(" elif v.get('method')=='turn/start':", " elif v.get('method')=='turn/start':\n  turn_count+=1; turn='turn-'+str(turn_count); success=True")
        fixture_text = fixture_text.replace("{'id':2,'result':{'turn':{'id':'turn'}}}", "{'id':v['id'],'result':{'turn':{'id':turn}}}")
        fixture_text = fixture_text.replace("'id':'turn'", "'id':turn").replace("'turnId':'turn'", "'turnId':turn")
        fixture.write_text(fixture_text)
        task = dict(driver.task(fixture, json.dumps([request, *transfers])), turn_limit=2)
        profile = driver.profile(task)
        digest = hashlib.sha256(json.dumps(operation, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        rights = [{'kind': 'cocoon', 'profile_sha256': sha(profile_path), 'operation_sha256': digest}]
        for plan in (upload, download):
            rights.append({'kind': 'cocoon', 'profile_sha256': sha(profile_path),
                'operation_sha256': hashlib.sha256(json.dumps(plan, sort_keys=True, separators=(',', ':')).encode()).hexdigest()})
        rights.append({'kind': 'read-input', 'path': str(source), 'sha256': sha(source)})
        resource = {'instance': str(instance), 'run': 'validate'}
        rights.extend([
            {'kind': 'run-candidate', **resource, 'transfer': transfers[1]['operation_id'], 'sha256': sha(source), 'seconds': 20},
            {'kind': 'accept', **resource},
            {'kind': 'artifact', **resource, 'output': 'artifact.bin'}])
        parent = driver.grant([profile, driver.profile(dict(task, turn_limit=1)), *rights], delegate=True)
        main_credential = driver.grant([profile, *rights], subject='primary', parent=parent)
        leaf = driver.grant([profile, driver.profile(dict(task, turn_limit=1))], subject='restricted', parent=parent)
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
            saved_results = {}
            try:
                for index in range(5 if args.cancel_validation else 6):
                    stream, _ = server.accept()
                    with stream:
                        stream.settimeout(10)
                        size, = struct.unpack('!I', receive_exact(stream, 4))
                        assert 0 < size <= 32768
                        body = json.loads(receive_exact(stream, size))
                        assert body['credential'] == ''
                        received.append(body['request'])
                        if index == 5:
                            assert body['operation']['operation'] == 'operation_status'
                            original = body['operation']['id']
                            assert original in saved_results
                            result = {'status': 'succeeded', 'result': saved_results[original]}
                        elif index == 0:
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
                        saved_results[body['request']] = result
                        response = json.dumps({'request': body['request'], 'caller': 'main',
                            'result': {'Ok': result}}).encode()
                        stream.sendall(struct.pack('!I', len(response)) + response)
            except Exception as error:
                errors.append(str(error))
        thread = threading.Thread(target=respond, daemon=True)
        if not live:
            thread.start()
        job = driver.approve({'op': 'harness', 'task': task}, main_credential)['id']
        deadline = time.monotonic() + 40
        while not (driver.service / 'jobs' / job / 'turn-result-1.json').exists():
            assert time.monotonic() < deadline
            time.sleep(.05)
        if not live:
            assert not errors, errors
            assert len(received) == 5 and len(set(received)) == 5 and received[0] != command
        candidate = driver.service / 'jobs' / job / 'cocoon-transfers' / transfers[1]['operation_id'] / 'candidate.bin'
        assert candidate.read_bytes() == binary
        assert driver.request({'op': 'harness-turn-result', 'id': job, 'turn': 1}, parent)['proposal'] == {'ok': True}
        selected = {'session': job, 'transfer': transfers[1]['operation_id'], 'sha256': sha(source)}
        submit = {'op': 'submit', **resource, 'candidate': selected, 'seconds': 20}
        driver.request(dict(submit, input=str(source)), parent, ok=False)
        driver.request(dict(submit, candidate=dict(selected, sha256='b'*64)), parent, ok=False)
        driver.request(submit, leaf, ok=False)
        preview = driver.request(submit, parent)
        candidate.write_bytes(b'tampered')
        driver.request(dict(submit, approve=preview['approval_sha256']), parent, ok=False)
        candidate.write_bytes(binary)
        validator = driver.approve(submit, parent)['id']
        if args.cancel_validation:
            marker = instance / 'runs' / 'validate' / 'job' / 'work' / 'validation-started'
            deadline = time.monotonic() + 30
            while not marker.exists():
                assert time.monotonic() < deadline
                time.sleep(.05)
            driver.approve({'op': 'cancel', 'id': job}, parent)
            driver.wait(validator, parent, 'cancelled')
            driver.wait(job, main_credential, 'cancelled')
            completion = json.loads((driver.service / 'jobs' / job / 'completion.json').read_text())
            assert completion['result']['termination_confirmed']
            validated = driver.request({'op': 'task', 'id': validator}, parent)
            assert validated['native']['termination_confirmed']
            assert driver.request({'op': 'grant-status'}, parent)['active']
            driver.request({'op': 'accept', **resource}, parent, ok=False)
            driver.request(submit, parent, ok=False)
            retained = driver.request({'op': 'cocoon-status', 'id': job, 'operation_id': command}, main_credential)
            assert retained['state'] == 'observed'
            server.close()
            if not live:
                thread.join(5)
                assert not thread.is_alive()
            (driver.root / 'report.json').write_text(json.dumps({'passed': True,
                'fixture_only': not bool(live), 'model_calls': 0, 'vm_execution_tested': bool(live),
                'source_cancellation_stops_validator': True, 'parent_grant_still_active': True,
                'no_late_acceptance': True, 'revoked_credential_observation': True}, indent=2))
            return
        driver.wait(validator, parent)
        validated = driver.request({'op': 'task', 'id': validator}, parent)
        assert validated['candidate_input']['transfer'] == selected['transfer']
        assert validated['authorization']['grant']['id'] == main_credential['id']
        assert validated['native']['termination_confirmed']
        read = {'op': 'artifact-read', **resource, 'output': 'artifact.bin'}
        driver.request(read, parent, ok=False)
        driver.approve({'op': 'accept', **resource}, parent)
        artifact = driver.request(read, parent)
        assert bytes(artifact['bytes']) == binary
        transfer_dir = driver.service / 'jobs' / job / 'cocoon-transfers' / transfers[1]['operation_id']
        chunk_id = hashlib.sha256((transfers[1]['operation_id'] + ':1').encode()).hexdigest()[:32]
        lost = transfer_dir / 'operations' / (chunk_id + '.result.json')
        assert lost.exists()
        lost.unlink()  # Host receipt loss; original Cocoon consumer identity remains durable.
        before = driver.request({'op': 'harness-connect', 'id': job}, parent)
        owner = (driver.service / 'jobs' / job / 'owner.json').read_bytes()
        driver.approve({'op': 'harness-continue', 'id': job, 'execution': before['execution'],
            'after_turn': 1, 'command': 'accepted-artifact', 'instruction': json.dumps([transfers[1], read])}, parent)
        driver.wait(job, parent)
        assert driver.request({'op': 'harness-turn-result', 'id': job, 'turn': 2}, parent)['proposal'] == {'ok': True}
        assert lost.exists()
        if not live:
            thread.join(15)
            assert not thread.is_alive() and not errors, errors
            assert len(received) == 6 and len(set(received)) == 6
        after = driver.request({'op': 'harness-connect', 'id': job}, parent)
        assert after['execution'] == before['execution']
        assert (driver.service / 'jobs' / job / 'owner.json').read_bytes() == owner
        assert driver.request({'op': 'harness-result', 'id': job}, parent)['proposal'] == {'ok': True}
        status = driver.request({'op': 'cocoon-status', 'id': job, 'operation_id': command}, parent)
        assert status['state'] == 'observed' and not status['artifact_accepted']
        connected = driver.request({'op': 'harness-connect', 'id': job}, parent)
        assert connected['cocoon_operations'] == [status]
        driver.request({'op': 'cocoon-status', 'id': job, 'operation_id': command}, leaf, ok=False)
        refused = driver.approve({'op': 'harness', 'task': dict(task, turn_limit=1)}, leaf)['id']
        driver.wait(refused, leaf)
        assert driver.request({'op': 'harness-result', 'id': refused}, leaf)['proposal'] == {'ok': False}
        (driver.root / 'report.json').write_text(json.dumps({'passed': True, 'fixture_only': not bool(live),
            'model_calls': 0, 'vm_execution_tested': bool(live), 'binary_transfer': True, 'single_thread_tool_sequence': True,
            'native_spool_custody': True, 'accepted_artifact_same_thread': True, 'lost_receipt_reconciled_without_replay': True, 'scoped_tool_dispatch': True, 'reconnected_receipt': True}, indent=2))
    finally:
        server.close()
        driver.close()


if __name__ == '__main__':
    main()

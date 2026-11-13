#!/usr/bin/env python3
"""Two original background owners, scoped steering and reconnect without replay."""
import argparse
import json
import signal
import subprocess
import sys
import time
from pathlib import Path

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
    try:
        fake = driver.root / 'fixture'
        fake.write_text('#!' + sys.executable + '\n' + '''import sys,json
if '--version' in sys.argv: print('codex-cli 0.166.0');sys.exit()
def send(v): print(json.dumps(v),flush=True)
for raw in sys.stdin:
 v=json.loads(raw)
 if v.get('method')=='initialize': send({'id':0,'result':{}})
 elif v.get('method')=='thread/start': send({'id':1,'result':{'thread':{'id':'thread'}}})
 elif v.get('method')=='turn/start':
  send({'id':2,'result':{'turn':{'id':'turn'}}})
  send({'method':'turn/started','params':{'threadId':'thread','turn':{'id':'turn'}}})
 elif v.get('method')=='turn/steer':
  assert v['params']['expectedTurnId']=='turn' and v['params']['threadId']=='thread'
  send({'id':v['id'],'result':{'turnId':'turn'}})
  if v['params']['input'][0]['text']=='finish':
   send({'method':'item/completed','params':{'threadId':'thread','turnId':'turn','item':{
    'type':'agentMessage','phase':'final_answer','text':'{"ok":true}'}}})
   send({'method':'turn/completed','params':{'threadId':'thread','turn':{'id':'turn','status':'completed'}}})
''')
        fake.chmod(0o700)
        task = driver.task(fake, 'hold until steered')
        task.pop('app_server')
        task['adapter'] = 'codex-app-server'
        profile = driver.profile(task)
        legacy = dict(task, app_server=True)
        legacy.pop('adapter')
        assert driver.profile(legacy) == profile
        parent = driver.grant([profile], delegate=True)
        children = [driver.grant([profile], subject=name, parent=parent) for name in ('first', 'second')]
        jobs = [driver.approve({'op': 'delegate', 'parent': 'document-study', 'task': task}, c)['id'] for c in children]
        for job in jobs:
            driver.ready(job)
        def connect(index, cursor=0):
            return driver.request({'op': 'harness-connect', 'id': jobs[index], 'cursor': cursor}, children[index])
        views = [connect(i) for i in range(2)]
        owners = [(driver.service / 'jobs' / job / 'owner.json').read_bytes() for job in jobs]
        assert all(v['execution'] and v['capabilities']['steer'] for v in views)
        driver.request({'op': 'harness-connect', 'id': jobs[0]}, children[1], ok=False)
        def steer(index, name, instruction):
            return {'op': 'harness-steer', 'id': jobs[index], 'execution': views[index]['execution'],
                    'command': name, 'instruction': instruction}
        driver.request(dict(steer(0, 'bad', 'edit'), execution=views[1]['execution']), children[0], ok=False)
        command = steer(0, 'update', 'keep going')
        driver.approve(command, children[0])
        deadline = time.monotonic() + 15
        while not any(c['state'] == 'acknowledged' for c in connect(0)['commands']):
            assert time.monotonic() < deadline
            time.sleep(.05)
        # Every controller request is a new connection. Restart the service too;
        # original task owners and their provider turns must survive unchanged.
        driver.process.send_signal(signal.SIGTERM)
        driver.process.wait(timeout=15)
        driver.process = subprocess.Popen([str(driver.host), 'daemon', 'serve', str(driver.service)],
                                          stdout=driver.log, stderr=driver.log)
        deadline = time.monotonic() + 10
        while not (driver.service / 'endpoint.json').exists():
            assert time.monotonic() < deadline and driver.process.poll() is None
            time.sleep(.05)
        for i in range(2):
            resumed = connect(i, views[i]['cursor'])
            assert resumed['execution'] == views[i]['execution']
            assert owners[i] == (driver.service / 'jobs' / jobs[i] / 'owner.json').read_bytes()
            assert all(e['cursor'] > views[i]['cursor'] for e in resumed['events'])
        driver.approve(command, children[0])  # Idempotent original command receipt.
        preview = driver.request(dict(command, instruction='different'), children[0])
        driver.request(dict(command, instruction='different', approve=preview['approval_sha256']), children[0], ok=False)
        driver.approve({'op': 'cancel', 'id': jobs[1]}, parent)
        driver.wait(jobs[1], children[1], 'cancelled')
        assert connect(1)['task']['outcome'] == 'cancelled'
        driver.request(steer(1, 'late', 'finish'), children[1], ok=False)
        driver.approve(steer(0, 'finish', 'finish'), children[0])
        driver.wait(jobs[0], children[0])
        assert driver.request({'op': 'harness-result', 'id': jobs[0]}, children[0])['proposal'] == {'ok': True}
        driver.request({'op': 'harness-connect', 'id': jobs[0], 'cursor': 1}, children[0], ok=False)
        # Simulate a missing acknowledgement after a known settled execution.
        (driver.service / 'jobs' / jobs[0] / 'ack-update.json').unlink()
        assert any(c == {'command': 'update', 'state': 'unknown'} for c in connect(0)['commands'])
        driver.approve(command, children[0])
        native = (driver.service / 'jobs' / jobs[0] / 'native-events.jsonl').read_text()
        assert native.count('steer-update') == 1
        assert any(e['event'] == 'turn-started' for e in driver.request({'op': 'activity', 'id': jobs[0]}, parent)['harness']['events'])
        (driver.root / 'report.json').write_text(json.dumps({'passed': True, 'fixture_only': True,
            'host_sha256': sha(driver.host), 'tasks': jobs, 'steering_acknowledged': True,
            'service_restart_same_owners': True, 'cancel_isolated': True,
            'foreign_scope_rejected': True, 'duplicate_command_not_replayed': True}, indent=2))
    finally:
        driver.close()


if __name__ == '__main__':
    main()

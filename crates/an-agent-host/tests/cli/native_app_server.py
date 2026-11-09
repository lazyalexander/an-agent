#!/usr/bin/env python3
"""Real local protocol processes; a deterministic fixture is not model evidence."""
import argparse
import json
from pathlib import Path
import sys
import time

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
        fake.write_text('#!' + sys.executable + '\n' + '''import sys,json,time
if '--version' in sys.argv: print('codex-cli 0.166.0');sys.exit()
def send(v): print(json.dumps(v),flush=True)
for raw in sys.stdin:
 v=json.loads(raw)
 if v.get('method')=='initialize': send({'id':0,'result':{}})
 elif v.get('method')=='thread/start':
  assert v['params']['ephemeral'] and len(v['params']['dynamicTools'])==1
  send({'id':1,'result':{'thread':{'id':'fixture-thread'}}})
 elif v.get('method')=='turn/start':
  send({'id':2,'result':{'turn':{'id':'fixture-turn'}}})
  send({'method':'turn/started','params':{'threadId':'fixture-thread','turn':{'id':'fixture-turn'}}})
  p=json.loads(v['params']['input'][0]['text'])
  if p.get('hold'): time.sleep(60)
  send({'id':99,'method':'item/tool/call','params':{'threadId':p.get('thread','fixture-thread'),'turnId':'fixture-turn',
   'tool':'host_request','namespace':None,'callId':'call-1','arguments':{'request':json.dumps(p['request'])}}})
 elif v.get('id')==99:
  send({'method':'item/completed','params':{'threadId':'fixture-thread','turnId':'fixture-turn',
   'item':{'type':'agentMessage','phase':'final_answer','text':json.dumps({'ok':v['result']['success']})}}})
  send({'method':'turn/completed','params':{'threadId':'fixture-thread','turn':{'id':'fixture-turn','status':'completed'}}})
''')
        fake.chmod(0o700)
        source = driver.root / 'input.txt'
        source.write_text('selected synthetic input')
        read = {'op': 'read-input', 'path': str(source), 'sha256': sha(source)}
        task = driver.task(fake, json.dumps({'request': read}))
        profile = driver.profile(task)
        permission = dict(read, kind='read-input')
        del permission['op']
        parent = driver.grant([profile, permission], delegate=True)
        leaf = driver.grant([profile], subject='restricted', parent=parent)
        driver.request({'op': 'derive-grant', 'scope': {'task': 'document-study', 'subject': 'escalation',
            'expires_ms': driver.expires, 'delegate': True, 'permissions': [permission]}}, leaf, ok=False)
        def run(task, cred, outcome='succeeded'):
            job = driver.approve({'op': 'harness', 'task': task}, cred)['id']
            driver.wait(job, cred, outcome)
            return job
        allowed = run(task, parent)
        assert driver.request({'op': 'harness-result', 'id': allowed}, parent)['proposal'] == {'ok': True}
        refused = run(task, leaf)
        assert driver.request({'op': 'harness-result', 'id': refused}, leaf)['proposal'] == {'ok': False}
        driver.request({'op': 'harness-result', 'id': allowed}, leaf, ok=False)
        foreign = run(dict(task, prompt=json.dumps({'request': read, 'thread': 'foreign'})), leaf, 'failed')
        assert not (driver.service / 'jobs' / foreign / 'decision.json').exists()
        for action in ('cancel', 'revoke'):
            cred = driver.grant([profile], subject=action, parent=parent)
            job = driver.approve({'op': 'harness', 'task': dict(task, prompt=json.dumps({'hold': True}))}, cred)['id']
            driver.ready(job)
            if action == 'cancel': driver.approve({'op': 'cancel', 'id': job}, parent)
            else: driver.approve({'op': 'revoke-grant', 'id': cred['id']}, parent)
            driver.wait(job, cred, 'cancelled')
            result = json.loads((driver.service / 'jobs' / job / 'completion.json').read_text())['result']
            assert result['termination_confirmed'] and not result['proposal_available']
        (driver.root / 'report.json').write_text(json.dumps({'passed': True, 'host_sha256': sha(driver.host),
            'fixture_only': True, 'model_calls': 0, 'attenuation': True, 'foreign_thread_refused': True,
            'cancel_after_turn_start': True, 'revoke_after_turn_start': True, 'automatic_retries': 0}, indent=2))
    finally:
        driver.close()


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Versioned context over the original owner; protocol fixture, not model qualification."""
import argparse
import json
import time
from pathlib import Path
import sys
from app_server_support import Driver
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
        fake.write_text('#!' + sys.executable + '\n' + '''import json,sys,uuid
if '--version' in sys.argv: print('codex-cli 0.166.0');sys.exit()
def send(v): print(json.dumps(v),flush=True)
def tool(number,request):
 send({'id':number,'method':'item/tool/call','params':{'threadId':'original','turnId':turn,
  'callId':str(uuid.uuid4()),'tool':'host_request','arguments':{'request':json.dumps(request)}}})
def finish():
 send({'method':'item/completed','params':{'threadId':'original','turnId':turn,'item':{'type':'agentMessage','phase':'final_answer','text':'{"ok":true}'}}})
 send({'method':'turn/completed','params':{'threadId':'original','turn':{'id':turn,'status':'completed'}}})
count=0; steered=False
for raw in sys.stdin:
 v=json.loads(raw)
 if v.get('method')=='initialize': send({'id':0,'result':{}})
 elif v.get('method')=='thread/start': send({'id':1,'result':{'thread':{'id':'original'}}})
 elif v.get('method')=='turn/start':
  assert v['params']['threadId']=='original'
  assert 'an-turn-context-v1' in v['params']['input'][0]['text']
  count+=1;turn='turn-'+str(count)
  send({'id':v['id'],'result':{'turn':{'id':turn}}})
  send({'method':'turn/started','params':{'threadId':'original','turn':{'id':turn}}})
  tool(99,{'op':'turn-context'})
 elif v.get('method')=='turn/steer':
  assert v['params']['expectedTurnId']==turn
  steered=True
  send({'id':v['id'],'result':{'turnId':turn}})
  tool(99,{'op':'turn-context'})
 elif v.get('id')==99:
  assert v['result']['success']
  context=json.loads(v['result']['contentItems'][0]['text'])['snapshot']
  tool(100,{'op':'turn-propose','context_revision':context['revision'],'proposal':str(uuid.uuid4()),'summary':'All artifacts accepted (untrusted claim)'})
 elif v.get('id')==100:
  assert v['result']['success']
  proposal=json.loads(v['result']['contentItems'][0]['text'])
  assert proposal['artifact_accepted'] is False and proposal['grants_authority'] is False
  if count>1 or steered: finish()
''')
        fake.chmod(0o700)
        task = dict(driver.task(fake, 'Propose a summary.', scope={
            'single_primary': True, 'turn_context': {'schema': 'an-turn-context-v1'}}), turn_limit=3)
        credential = driver.grant([driver.profile(task)])
        job = driver.approve({'op': 'harness', 'task': task}, credential)['id']
        directory = driver.service / 'jobs' / job

        def completed(number):
            deadline = time.monotonic() + 30
            while not (directory / f'turn-result-{number}.json').exists():
                assert time.monotonic() < deadline
                time.sleep(.05)
            return driver.request({'op': 'harness-connect', 'id': job}, credential)

        deadline = time.monotonic() + 30
        while not list((directory / 'context-proposals').glob('*.json')):
            assert time.monotonic() < deadline
            time.sleep(.05)
        active = driver.request({'op': 'harness-connect', 'id': job}, credential)
        initial_revision = active['context']['snapshot']['revision']
        driver.approve({'op': 'harness-steer', 'id': job, 'execution': active['execution'],
            'command': 'supplement', 'instruction': 'Preserve the original source facts.',
            'context_revision': initial_revision}, credential)
        first = completed(1)
        assert first['context']['snapshot']['parent'] == initial_revision
        assert first['context']['snapshot']['turn'] == 1
        assert first['context']['delivery']['state'] == 'acknowledged'
        owner = (directory / 'owner.json').read_bytes()
        revision = first['context']['snapshot']['revision']
        command = {'op': 'harness-continue', 'id': job, 'execution': first['execution'],
                   'after_turn': 1, 'command': 'next', 'instruction': 'Continue independently checking facts.'}
        driver.request(command, credential, ok=False)
        driver.request(dict(command, context_revision='0' * 64), credential, ok=False)
        command['context_revision'] = revision
        driver.approve(command, credential)
        second = completed(2)
        assert second['execution'] == first['execution']
        assert (directory / 'owner.json').read_bytes() == owner
        context = second['context']['snapshot']
        assert context['parent'] == revision and context['turn'] == 2
        assert context['summary']['artifact_accepted'] is False
        assert context['limits'] == first['context']['snapshot']['limits']
        assert context['spool_bindings'] == []
        assert driver.approve(command, credential)['replayed'] is False
        driver.request(dict(command, instruction='Different bytes'), credential, ok=False)
        # Lose only this fixture's acknowledgement: reconnect must not fabricate it.
        (directory / 'ack-next.json').unlink()
        unknown = driver.request({'op': 'harness-connect', 'id': job}, credential)
        assert unknown['context']['delivery']['state'] == 'unknown'
        driver.request(dict(command, after_turn=2, command='third', context_revision=context['revision']), credential, ok=False)
        assert len(list(directory.glob('sent-next.json'))) == 1
        driver.approve({'op': 'cancel', 'id': job}, credential)
        driver.wait(job, credential, 'cancelled')
        driver.request(dict(command, after_turn=2, command='after-cancel', context_revision=context['revision']), credential, ok=False)
        (driver.root / 'report.json').write_text(json.dumps({'passed': True, 'fixture_only': True,
            'same_owner': True, 'same_turn_steering': True, 'stale_revision_denied': True, 'summary_not_acceptance': True,
            'unknown_not_replayed': True, 'cancelled': True}, indent=2))
    finally:
        driver.close()


if __name__ == '__main__':
    main()

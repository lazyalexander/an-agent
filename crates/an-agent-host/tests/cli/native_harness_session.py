#!/usr/bin/env python3
"""Original-thread continuation, cumulative usage and authorized disclosures."""
import argparse
import json
import signal
import subprocess
import sys
import time
from pathlib import Path
from types import SimpleNamespace

from app_server_support import Driver, sha
from harness_benchmark import work_case
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
        fake.write_text('#!' + sys.executable + '\n' + '''import json,sys
if '--version' in sys.argv: print('codex-cli 0.166.0');sys.exit()
count=0
def send(v): print(json.dumps(v),flush=True)
def finish(proposal):
 for _ in range(2):
  send({'method':'thread/tokenUsage/updated','params':{'threadId':'original','turnId':turn,
   'tokenUsage':{'total':{'inputTokens':10*count,'cachedInputTokens':4*count,'outputTokens':2*count,
   'reasoningOutputTokens':count,'totalTokens':12*count}}}})
 send({'method':'item/completed','params':{'threadId':'original','turnId':turn,
  'item':{'type':'agentMessage','phase':'final_answer','text':json.dumps(proposal)}}})
 send({'method':'turn/completed','params':{'threadId':'original','turn':{'id':turn,'status':'completed'}}})
for raw in sys.stdin:
 v=json.loads(raw)
 if v.get('method')=='initialize': send({'id':0,'result':{}})
 elif v.get('method')=='thread/start': send({'id':1,'result':{'thread':{'id':'original'}}})
 elif v.get('method')=='turn/start':
  assert v['params']['threadId']=='original'
  count+=1; turn='turn-'+str(count)
  send({'id':v['id'],'result':{'turn':{'id':turn}}})
  send({'method':'turn/started','params':{'threadId':'original','turn':{'id':turn}}})
  text=v['params']['input'][0]['text']
  if 'order' in v['params']['outputSchema']['properties']:
   if count==2:
    finish({'order':order,'results':json.loads(text[text.index('['):])})
    continue
   text=text[text.index('{'):]
  send({'id':99,'method':'item/tool/call','params':{'threadId':'original','turnId':turn,
   'tool':'host_request','namespace':None,'callId':'read','arguments':{'request':text}}})
 elif v.get('id')==99:
  assert v['result']['success']
  text=json.loads(v['result']['contentItems'][0]['text'])['text']
  if text=='selected knowledge':
   finish({'ok':True})
  else:
   order=[stage['id'] for stage in json.loads(text)['stages']][::-1]
   finish({'order':order,'results':[]})
''')
        fake.chmod(0o700)
        source = driver.root / 'knowledge.txt'
        source.write_text('selected knowledge')
        read = {'op': 'read-input', 'path': str(source), 'sha256': sha(source), 'encoding': 'utf-8'}
        task = dict(driver.task(fake, json.dumps(read)), turn_limit=2)
        profile = driver.profile(task)
        assert profile != driver.profile(driver.task(fake, json.dumps(read)))
        permission = {'kind': 'read-input', 'path': str(source), 'sha256': sha(source)}
        parent = driver.grant([profile, permission], delegate=True)
        main = driver.grant([profile, permission], parent=parent)
        foreign = driver.grant([profile], subject='foreign', parent=parent)
        job = driver.approve({'op': 'harness', 'task': task}, main)['id']
        deadline = time.monotonic() + 20
        while not (driver.service / 'jobs' / job / 'turn-result-1.json').exists():
            assert time.monotonic() < deadline
            time.sleep(.05)
        first = driver.request({'op': 'harness-turn-result', 'id': job, 'turn': 1}, main)
        driver.request({'op': 'harness-turn-result', 'id': job, 'turn': 1}, foreign, ok=False)
        driver.request({'op': 'harness-result', 'id': job}, main, ok=False)
        view = driver.request({'op': 'harness-connect', 'id': job}, main)
        assert view['turns'] == {'completed': 1, 'limit': 2, 'active': None}
        driver.request({'op': 'harness-steer', 'id': job, 'execution': view['execution'],
                        'command': 'idle-steer', 'instruction': 'Do not attach to a future turn'}, main, ok=False)
        # Freeze the race where an admitted command becomes visible after its turn ends.
        directory = driver.service / 'jobs' / job
        (directory / 'commands').mkdir(exist_ok=True)
        stale = directory / 'commands/stale.pending'
        stale.write_text(json.dumps({'plan': {
            'operation': 'steer-harness', 'command': 'stale', 'execution': view['execution'],
            'turn': first['turn']}, 'sequence': 0, 'instruction': 'Never deliver this to turn two'}))
        stale.rename(stale.with_suffix('.json'))
        deadline = time.monotonic() + 10
        while not (directory / 'ack-stale.json').exists():
            assert time.monotonic() < deadline
            time.sleep(.05)
        assert json.loads((directory / 'ack-stale.json').read_text())['state'] == 'rejected'
        assert not (directory / 'sent-stale.json').exists()
        owner = (driver.service / 'jobs' / job / 'owner.json').read_bytes()
        driver.process.send_signal(signal.SIGTERM)
        driver.process.wait(timeout=15)
        driver.process = subprocess.Popen([str(driver.host), 'daemon', 'serve', str(driver.service)], stdout=driver.log, stderr=driver.log)
        deadline = time.monotonic() + 10
        while not (driver.service / 'endpoint.json').exists():
            assert time.monotonic() < deadline
            time.sleep(.05)
        command = {'op': 'harness-continue', 'id': job, 'execution': view['execution'],
                   'after_turn': 1, 'command': 'next', 'instruction': json.dumps(read)}
        driver.approve(command, main)
        driver.approve(command, main)
        driver.wait(job, main)
        driver.approve(command, main)
        second = driver.request({'op': 'harness-turn-result', 'id': job, 'turn': 2}, main)
        assert first['thread'] == second['thread']
        assert owner == (driver.service / 'jobs' / job / 'owner.json').read_bytes()
        activity = driver.request({'op': 'activity', 'id': job}, main)
        metrics = activity['harness']['metrics']
        assert metrics['usage']['updates'] == 4
        assert metrics['usage']['total_tokens'] == 24
        assert metrics['usage']['sessions'] == 1
        assert metrics['disclosure']['reads'] == 2
        assert metrics['disclosure']['total_bytes'] == 36
        assert metrics['disclosure']['unique_bytes'] == 18
        native = (driver.service / 'jobs' / job / 'native-events.jsonl').read_text()
        assert native.count('"id":"continue-next"') == 1
        final = driver.request({'op': 'harness-result', 'id': job}, main)
        assert final['proposal'] == {'ok': True} and final['result']['termination_confirmed']
        driver.request(dict(command, after_turn=2, command='beyond-limit'), main, ok=False)
        cancelled = driver.approve({'op': 'harness', 'task': task}, main)['id']
        deadline = time.monotonic() + 20
        while not (driver.service / 'jobs' / cancelled / 'turn-result-1.json').exists():
            assert time.monotonic() < deadline
            time.sleep(.05)
        driver.approve({'op': 'cancel', 'id': cancelled})
        driver.wait(cancelled, main, 'cancelled')
        completion = json.loads((driver.service / 'jobs' / cancelled / 'completion.json').read_text())
        assert completion['result']['termination_confirmed']
        assert not (driver.service / 'jobs' / cancelled / 'turn-start-2.json').exists()
        driver.request(dict(command, id=cancelled), main, ok=False)
        _, programs = work_case(driver, SimpleNamespace(codex=fake, model=None, scenario='work_shape.parallel_tools'))
        assert programs['accepted_values'] == [22, 14, 10], programs
        (driver.root / 'report.json').write_text(json.dumps({'passed': True, 'fixture_only': True,
            'host_sha256': sha(driver.host), 'single_thread': True, 'restart_same_owner': True,
            'continuation_not_replayed': True, 'idle_steering_refused': True,
            'idle_cancellation_released': True, 'reversed_program_order_accepted': True,
            'metrics': metrics}, indent=2))
    finally:
        driver.close()


if __name__ == '__main__':
    main()


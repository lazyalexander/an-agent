#!/usr/bin/env python3
"""Recover product references through the real daemon after lost enqueue replies."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from app_server_support import Driver
from host_qualification import require_native_workspace


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host',type=Path,required=True)
    parser.add_argument('--evidence',type=Path,required=True)
    parser.add_argument('--repetitions',type=int,default=10)
    args=parser.parse_args();require_native_workspace()
    assert 1<=args.repetitions<=10
    driver=Driver(args.host,args.evidence);page=None;results=[]
    try:
        fixture=driver.root/'fixture'
        fixture.write_text('#!'+sys.executable+'\n'+'''import json,sys
if '--version' in sys.argv: print('codex-cli 0.166.0');sys.exit()
def send(v): print(json.dumps(v),flush=True)
for line in sys.stdin:
 v=json.loads(line)
 if v.get('method')=='initialize': send({'id':v['id'],'result':{}})
 elif v.get('method')=='thread/start': send({'id':v['id'],'result':{'thread':{'id':'original'}}})
 elif v.get('method')=='turn/start':
  send({'id':v['id'],'result':{'turn':{'id':'original-turn'}}})
  send({'method':'turn/started','params':{'threadId':'original','turn':{'id':'original-turn'}}})
  send({'method':'item/completed','params':{'threadId':'original','turnId':'original-turn','item':{'type':'agentMessage','phase':'final_answer','text':'{"ok":true}'}}})
  send({'method':'turn/completed','params':{'threadId':'original','turn':{'id':'original-turn','status':'completed'}}})
''');fixture.chmod(0o700)
        page=subprocess.Popen([str(driver.host),'app','serve',str(driver.service)],stdout=driver.log,stderr=driver.log)
        endpoint=driver.service/'application/endpoint.json';deadline=time.monotonic()+15
        while True:
            assert page.poll() is None and time.monotonic()<deadline
            try:
                base,token=json.loads(endpoint.read_text())['url'].split('/#');break
            except (FileNotFoundError,json.JSONDecodeError):time.sleep(.025)
        def call(value):
            request=urllib.request.Request(base+'/action',data=json.dumps(value).encode(),headers={
                'Origin':base,'Authorization':'Bearer '+token,'Content-Type':'application/json'})
            try:
                with urllib.request.urlopen(request,timeout=40) as response:return json.load(response)
            except urllib.error.HTTPError as error:
                raise AssertionError((value['op'],error.code,error.read().decode())) from error
        for boundary in ('before-enqueue','lost-reply','daemon-reconnect'):
            for repeat in range(args.repetitions):
                identity=str(uuid.uuid4());root=driver.service/'application/tasks'/identity;root.mkdir(mode=0o700)
                (root/'intents').mkdir(mode=0o700)
                (root/'recovery-version.json').write_text('{"version":1}')
                (root/'state.json').write_text('{"state":"unknown","session":null,"steps":{}}')
                task=dict(driver.task(fixture,'Return ok.',scope={'single_primary':True}),turn_limit=3,wall_seconds=90)
                credential=driver.approve({'op':'grant','scope':{'task':identity,'subject':'primary','delegate':False,
                    'expires_ms':int(time.time()*1000)+120000,'permissions':[driver.profile(task)]}})['credential']
                (root/'credential.json').write_text(json.dumps(credential))
                request={'op':'harness','task':task};preview=driver.request(request,credential)
                (root/'intents/primary.json').write_text(json.dumps({'kind':'harness','request':preview['plan']['task_sha256'],'grant':credential['id']}))
                original=None
                if boundary!='before-enqueue':
                    original=driver.request(dict(request,approve=preview['approval_sha256']),credential)['id']
                    driver.ready(original)
                if boundary=='daemon-reconnect':
                    driver.process.terminate();driver.process.wait(timeout=15)
                    driver.process=subprocess.Popen([str(driver.host),'daemon','serve',str(driver.service)],stdout=driver.log,stderr=driver.log)
                    deadline=time.monotonic()+15
                    while not (driver.service/'endpoint.json').exists():
                        assert driver.process.poll() is None and time.monotonic()<deadline
                        time.sleep(.025)
                before=sorted(p.name for p in (driver.service/'jobs').iterdir());started=time.monotonic()
                call({'op':'delivery-cancel','id':identity})
                deadline=started+30
                while True:
                    state=call({'op':'delivery-reconcile','id':identity})['state']
                    if state['state']=='cancelled':break
                    assert time.monotonic()<deadline,state
                    time.sleep(.05)
                elapsed=time.monotonic()-started
                assert state['termination_confirmed'] and elapsed<=30
                assert state.get('session')==original,state
                assert sorted(p.name for p in (driver.service/'jobs').iterdir())==before
                if original:
                    assert driver.request({'op':'task','id':original})['outcome']=='cancelled'
                    completion=json.loads((driver.service/'jobs'/original/'completion.json').read_text())
                    assert completion['result']['termination_confirmed']
                assert 'revoked' in driver.request(request,credential,ok=False)
                assert call({'op':'delivery-reconcile','id':identity})['state']==state
                results.append({'boundary':boundary,'repeat':repeat+1,'seconds':elapsed,'original_job':original})
        # A start intent with no original stop authority must remain unknown.
        identity=str(uuid.uuid4());root=driver.service/'application/tasks'/identity;root.mkdir(mode=0o700)
        for name,value in [('recovery-version',{'version':1}),('state',{'state':'unknown'}),('start-intent',{})]:
            (root/(name+'.json')).write_text(json.dumps(value))
        assert call({'op':'delivery-reconcile','id':identity})['state']['state']=='unknown'
        (driver.root/'report.json').write_text(json.dumps({'passed':True,'fixture_only':True,
            'real_daemon':True,'automatic_replays':0,'missing_authority_retained':True,'cases':results},indent=2))
    finally:
        if page is not None and page.poll() is None:page.terminate();page.wait(timeout=15)
        driver.close()


if __name__=='__main__':main()

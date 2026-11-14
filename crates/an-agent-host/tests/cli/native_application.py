#!/usr/bin/env python3
"""Cross-repository offline import through the real product HTTP boundary."""
import argparse
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import time
import sys
import urllib.error
import urllib.request
from host_qualification import require_native_workspace


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('host','loom','evidence'):parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args();require_native_workspace()
    host=args.host.resolve(strict=True);loom=args.loom.resolve(strict=True)
    root=args.evidence.resolve();root.mkdir(mode=0o700)
    source=root/'source';source.mkdir();(source/'NOTICE').write_bytes(b'licensed fixture')
    manifest={'schema':'an-offline-runtime-v1','name':'fixture','version':'1','platform':'macos-aarch64',
        'licenses':['NOTICE'],'components':{'runtime':'NOTICE'},'files':{'NOTICE':{
        'sha256':hashlib.sha256(b'licensed fixture').hexdigest(),'bytes':16,'executable':False}}}
    selection=root/'selection.json';selection.write_text(json.dumps(manifest));archive=root/'fixture.tar.gz'
    subprocess.run([str(loom),'pack-runtime',str(source),str(selection),str(archive)],check=True,stdout=subprocess.DEVNULL)
    service=root/'service';subprocess.run([str(host),'daemon','init',str(service),'1'],check=True,stdout=subprocess.DEVNULL)
    processes=[]
    log=(root/'process.log').open('w')
    try:
        for command,marker in [('daemon',service/'endpoint.json'),('app',service/'application/endpoint.json')]:
            process=subprocess.Popen([str(host),command,'serve',str(service)],stdout=log,stderr=log);processes.append(process)
            deadline=time.monotonic()+15
            while not marker.exists():
                assert process.poll() is None and time.monotonic()<deadline
                time.sleep(.02)
        endpoint=json.loads(marker.read_text())['url'];base,token=endpoint.split('/#')
        def call(path,body=None,authorized=True,origin=None,expected=200):
            headers={'Origin':origin or base}
            if authorized:headers['Authorization']='Bearer '+token
            data=body if isinstance(body,bytes) else json.dumps(body).encode() if body else None
            request=urllib.request.Request(base+path,data=data,headers=headers)
            try:
                with urllib.request.urlopen(request,timeout=15) as response:
                    status=response.status;payload=response.read()
            except urllib.error.HTTPError as error:status=error.code;payload=error.read()
            assert status==expected,(path,status,payload)
            return json.loads(payload)
        call('/view',authorized=False,expected=400)
        call('/action',{'op':'environment-preview','upload':'invalid'},origin='http://foreign.test',expected=400)
        upload=call('/upload',archive.read_bytes())['upload']
        preview=call('/action',{'op':'environment-preview','upload':upload})
        call('/action',{'op':'environment-import','upload':upload,'approve':'wrong'},expected=400)
        action={'op':'environment-import','upload':upload,'approve':preview['approval_sha256']}
        assert not call('/action',action)['reused']
        assert call('/action',action)['reused']
        assert len(call('/view')['environments'])==1
        call('/action',{'op':'daemon-request','request':{'op':'show'}},expected=400)
        fixture=root/'fixture'
        fixture.write_text('#!'+sys.executable+'\n'+'import sys,json\nif \'--version\' in sys.argv:print(\'codex-cli 0.166.0\');sys.exit()\ncount=0\ndef send(v):print(json.dumps(v),flush=True)\nfor raw in sys.stdin:\n v=json.loads(raw)\n if v.get(\'method\')==\'initialize\':send({\'id\':v[\'id\'],\'result\':{}})\n elif v.get(\'method\')==\'thread/start\':send({\'id\':v[\'id\'],\'result\':{\'thread\':{\'id\':\'product-fixture\'}}})\n elif v.get(\'method\')==\'turn/start\':\n  count+=1;turn=\'turn-\'+str(count)\n  send({\'id\':v[\'id\'],\'result\':{\'turn\':{\'id\':turn}}})\n  send({\'method\':\'turn/started\',\'params\':{\'threadId\':\'product-fixture\',\'turn\':{\'id\':turn}}})\n  send({\'method\':\'item/completed\',\'params\':{\'threadId\':\'product-fixture\',\'turnId\':turn,\'item\':{\'type\':\'agentMessage\',\'phase\':\'final_answer\',\'text\':\'{"ok":true}\'}}})\n  send({\'method\':\'turn/completed\',\'params\':{\'threadId\':\'product-fixture\',\'turn\':{\'id\':turn,\'status\':\'completed\'}}})\n')
        fixture.chmod(0o700)
        request_file=root/'request.json'
        def native(value,credential=None):
            request_file.write_text(json.dumps({'credential':credential,'request':value} if credential else value))
            return json.loads(subprocess.check_output([str(host),'daemon','task-request' if credential else 'request',str(service),str(request_file)],text=True))
        def approved(value,credential=None):
            preview=native(value,credential)
            return native(dict(value,approve=preview['approval_sha256']),credential)
        task={'app_server':True,'label':'product-control-fixture','binary':str(fixture),
              'binary_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'prompt':'Return ok.',
              'schema':{'type':'object','properties':{'ok':{'type':'boolean'}},'required':['ok'],'additionalProperties':False},
              'scope':{},'model':None,'mcp':None,'turn_limit':2}
        permission=native({'op':'harness-permission','task':task})
        credential=approved({'op':'grant','scope':{'task':'product-fixture','subject':'main',
            'expires_ms':int(time.time()*1000)+300000,'delegate':False,'permissions':[permission]}})['credential']
        def wait_for(job,settled=False):
            deadline=time.monotonic()+30
            while time.monotonic()<deadline:
                value=call('/action',{'op':'task-view','id':job})
                if (value['task']['state']=='settled') if settled else (value['session']['turns']['completed']==1):return value
                time.sleep(.05)
            raise AssertionError('Original task did not reach expected state')
        job=approved({'op':'harness','task':task},credential)['id']
        value=wait_for(job)
        supplement={'op':'task-control','control':'supplement','id':job,'execution':value['session']['execution'],
                    'after_turn':1,'command':'user-supplement','instruction':'Preserve all source facts.'}
        preview=call('/action',supplement)
        call('/action',dict(supplement,approve=preview['approval_sha256']))
        assert wait_for(job,True)['task']['outcome']=='succeeded'
        job=approved({'op':'harness','task':task},credential)['id'];wait_for(job)
        cancel={'op':'task-control','control':'cancel','id':job}
        preview=call('/action',cancel);call('/action',dict(cancel,approve=preview['approval_sha256']))
        assert wait_for(job,True)['task']['outcome']=='cancelled'
        (root/'report.json').write_text(json.dumps({'passed':True,'offline_import':True,'same_origin_enforced':True,
            'arbitrary_daemon_requests_denied':True,'supplement_and_cancel':True,'host_sha256':hashlib.sha256(host.read_bytes()).hexdigest()},indent=2))
    finally:
        for process in reversed(processes):
            if process.poll() is None:process.send_signal(signal.SIGTERM)
            process.wait(timeout=15)
        log.close()


if __name__=='__main__':main()

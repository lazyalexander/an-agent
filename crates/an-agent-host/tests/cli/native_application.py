#!/usr/bin/env python3
"""Cross-repository offline import through the real product HTTP boundary."""
import argparse
import hashlib
import json
from pathlib import Path
import signal
import subprocess
import time
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
        (root/'report.json').write_text(json.dumps({'passed':True,'offline_import':True,'same_origin_enforced':True,
            'arbitrary_daemon_requests_denied':True,'host_sha256':hashlib.sha256(host.read_bytes()).hexdigest()},indent=2))
    finally:
        for process in reversed(processes):
            if process.poll() is None:process.send_signal(signal.SIGTERM)
            process.wait(timeout=15)
        log.close()


if __name__=='__main__':main()

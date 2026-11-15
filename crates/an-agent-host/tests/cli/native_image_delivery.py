#!/usr/bin/env python3
"""Exercise private image management and independent real-model tasks via the product UI API."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import urllib.request
import urllib.error
import uuid
from host_qualification import require_native_workspace


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for key in ('host','bundle','evidence'):parser.add_argument('--'+key,type=Path,required=True)
    parser.add_argument('--rounds',type=int,choices=range(1,4),default=3)
    parser.add_argument('--continuation',action='store_true')
    args=parser.parse_args();require_native_workspace()
    root=args.evidence.resolve();root.mkdir(mode=0o700);service=root/'service';host=args.host.resolve(strict=True)
    log=(root/'page.log').open('w');page=None;base=token=None;records=[]
    def start():
        nonlocal page,base,token
        old=None;endpoint=service/'application/endpoint.json'
        if endpoint.exists():old=json.loads(endpoint.read_text())['url']
        page=subprocess.Popen([str(host),'app','serve',str(service)],stdout=log,stderr=log)
        deadline=time.monotonic()+20
        while True:
            assert page.poll() is None and time.monotonic()<deadline
            try:
                url=json.loads(endpoint.read_text())['url']
                if url!=old:base,token=url.split('/#');return
            except (FileNotFoundError,json.JSONDecodeError):pass
            time.sleep(.025)
    def close_page():
        if page is not None and page.poll() is None:page.terminate();page.wait(timeout=15)
    def call(value=None,path='/action',raw=None):
        data=raw if raw is not None else json.dumps(value).encode() if value is not None else None
        request=urllib.request.Request(base+path,data=data,headers={'Authorization':'Bearer '+token,'Origin':base,'Content-Type':'application/json' if raw is None else 'application/octet-stream'})
        try:
            with urllib.request.urlopen(request,timeout=300) as response:return json.load(response)
        except urllib.error.HTTPError as error:raise AssertionError(error.read().decode()) from error
    def image(action,**fields):
        identity=str(uuid.uuid4());value={'op':'image-preview','id':identity,'environment':environment,'action':action,**fields}
        preview=call(value);request={**value,'op':'image-submit','approve':preview['approval_sha256']}
        assert 'credential' not in json.dumps(preview),'Management credential disclosed in preview'
        assert call(request)['id']==identity
        assert call(request)['id']==identity
        deadline=time.monotonic()+240
        while True:
            selected=next(i for i in call(path='/view')['images'] if i['id']==identity)
            if selected['state'] in ('succeeded','failed','cancelled'):break
            assert time.monotonic()<deadline,selected
            time.sleep(.1)
        assert selected['state']=='succeeded',selected
        assert call(request)==selected
        records.append(selected);return selected
    def reference(record):
        result=record['receipt']['result'];return {'image':result['image'],'sha256':result['sha256']}
    def delivery(reference_value,name,resume_task=None):
        close_page();path=root/(name+'-image.json');path.write_text(json.dumps(reference_value))
        destination=root/name
        command=[sys.executable,'-B',str(Path(__file__).with_name('native_product_delivery.py')),'--host',str(host),'--bundle',str(args.bundle.resolve()),'--reuse-service',str(service),'--image-reference',str(path),'--evidence',str(destination),'--restart-page']
        if resume_task:command+=['--resume-task',resume_task,'--source-revision']
        started=time.monotonic()
        result=subprocess.run(command,stdout=log,stderr=log,timeout=900)
        assert result.returncode==0,('Real product image delivery failed',destination)
        start();report=json.loads((destination/'report.json').read_text());assert report['passed'] and report['real_model']
        task=service/'application/tasks'/report['task'];plan=json.loads((task/'plan.json').read_text())
        setup=json.loads((task/'setup.json').read_text());branch=json.loads((task/'image-branch-result.json').read_text())
        report['elapsed_seconds']=time.monotonic()-started
        if resume_task:assert plan['project']['source_task']==resume_task
        assert plan['image']['image']==reference_value['image'] and branch['state']=='succeeded'
        assert branch['result']['vm']==setup['vm'] and setup['image']==reference_value['image']
        return report,setup
    try:
        start();upload=call(path='/upload',raw=args.bundle.read_bytes())['upload']
        preview=call({'op':'environment-preview','upload':upload})
        environment=call({'op':'environment-import','upload':upload,'approve':preview['approval_sha256']})['id']
        candidate=image('save_environment');approved=image('promote',image=reference(candidate));base_image=reference(approved)
        standalone=image('branch',image=base_image)
        image('rollback',image=base_image,branch=standalone['id'])
        cycles=[]
        for cycle in range(args.rounds):
            original,source=delivery(base_image,f'environment-task-{cycle+1}')
            project=image('save_project',task=original['task']);assert project['kind']=='project'
            accepted=image('promote',image=reference(project));assert accepted['kind']=='project'
            resumed,target=delivery(reference(accepted),f'project-task-{cycle+1}',original['task'] if args.continuation else None)
            assert source['vm']!=target['vm'] and original['task']!=resumed['task']
            cycles.append({'environment_task':original['task'],'project_task':resumed['task'],'source_vm':source['vm'],'independent_vm':target['vm'],'source_products':original['products'],'products':resumed['products'],'environment_seconds':original['elapsed_seconds'],'project_seconds':resumed['elapsed_seconds'],'resume_task':original['task'] if args.continuation else None})
            (root/'checkpoint.json').write_text(json.dumps(cycles,indent=2))
            print(json.dumps({'cycle':cycle+1,'passed':True}),flush=True)
        (root/'report.json').write_text(json.dumps({'passed':True,'rounds':args.rounds,'real_model':True,'product_http':True,'host_sha256':hashlib.sha256(host.read_bytes()).hexdigest(),'cycles':cycles,'image_operations':records,'memory_restore_qualified':False,'automatic_replays':0},indent=2))
    finally:
        close_page()
        endpoint=service/'endpoint.json'
        if endpoint.exists():
            pid=json.loads(endpoint.read_text()).get('pid')
            if pid:
                try:os.kill(pid,signal.SIGTERM)
                except ProcessLookupError:pass
                deadline=time.monotonic()+15
                while endpoint.exists() and time.monotonic()<deadline:time.sleep(.025)
                assert not endpoint.exists(),'Original daemon shutdown remains unconfirmed'
        log.close()


if __name__=='__main__':main()

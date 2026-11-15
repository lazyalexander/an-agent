#!/usr/bin/env python3
"""Start with an empty service and exercise the actual product HTTP document path."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import urllib.request
import urllib.error
import uuid
from document_benchmark import make_pdf
from host_qualification import require_native_workspace


def claim_belongs(path,root):
    try:return json.loads(path.read_text()).get('session')==str(root)
    except (FileNotFoundError,json.JSONDecodeError):return False


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('host','bundle','evidence'):parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--reuse-service',type=Path)
    parser.add_argument('--image-reference',type=Path)
    parser.add_argument('--prepare-image',action='store_true')
    parser.add_argument('--resume-task')
    parser.add_argument('--source-revision',action='store_true')
    parser.add_argument('--cancel-when-image-branch',action='store_true')
    parser.add_argument('--clarification',action='store_true')
    parser.add_argument('--cancel',action='store_true')
    parser.add_argument('--cancel-preparing',action='store_true')
    parser.add_argument('--cancel-when-clarifying',action='store_true')
    parser.add_argument('--restart-page',action='store_true')
    parser.add_argument('--lose-controller',action='store_true')
    parser.add_argument('--lose-controller-at',choices=('setup','validator','image-branch'))
    parser.add_argument('--drop-execution-projection',action='store_true')
    parser.add_argument('--drop-clarification-receipt',action='store_true')
    args=parser.parse_args();require_native_workspace()
    if args.lose_controller_at or args.drop_execution_projection:
        assert args.cancel,'Fault recovery cases require explicit cancellation'
        args.lose_controller=True
    host=args.host.resolve(strict=True);host_digest=hashlib.sha256(host.read_bytes()).hexdigest();root=args.evidence.resolve();root.mkdir(mode=0o700)
    service=args.reuse_service.resolve(strict=True) if args.reuse_service else root/'service'
    existing_jobs={p.name for p in (service/'jobs').iterdir()} if service.exists() else set()
    log=(root/'page.log').open('w')
    previous_url=None
    retained_endpoint=service/'application/endpoint.json'
    if retained_endpoint.exists():
        try:previous_url=json.loads(retained_endpoint.read_text())['url']
        except json.JSONDecodeError:pass
    page=subprocess.Popen([str(host),'app','serve',str(service)],stdout=log,stderr=log)
    endpoint=service/'application/endpoint.json';task=None;base=None;token=None;observation_reconnects=0
    def connect(previous=None):
        nonlocal base,token
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            if endpoint.exists():
                try:url=json.loads(endpoint.read_text())['url']
                except (FileNotFoundError,json.JSONDecodeError):url=None
                if url and url!=previous:base,token=url.split('/#');return url
            assert page.poll() is None
            time.sleep(.05)
        raise AssertionError('Product page unavailable')
    def call(value=None,path='/action',raw=None):
        nonlocal observation_reconnects
        data=raw if raw is not None else json.dumps(value).encode() if value is not None else None
        request=urllib.request.Request(base+path,data=data,headers={'Origin':base,'Authorization':'Bearer '+token,'Content-Type':'application/json' if raw is None else 'application/octet-stream'})
        for attempt in range(3):
            try:
                with urllib.request.urlopen(request,timeout=300) as response:return json.load(response)
            except urllib.error.HTTPError as error:
                raise AssertionError((path,error.code,error.read().decode())) from error
            except (ConnectionResetError,urllib.error.URLError):
                if value is None or value.get('op')!='delivery-view' or attempt==2:raise
                assert page.poll() is None,'Original page owner exited'
                observation_reconnects+=1;time.sleep(.05)
    def approve(value):
        preview=call(value);return call(dict(value,approve=preview['approval_sha256']))
    def image_status(task_root):
        config=json.loads((task_root/'plan.json').read_text());environment=Path(config['environment'])
        manifest=json.loads((environment/'runtime.json').read_text())
        response=subprocess.run([str(environment/manifest['components']['cocoon']),'host',config['cocoon_state'],'image-status',str(task_root/'image-branch-query.json')],capture_output=True,text=True,timeout=10)
        return json.loads(response.stdout) if response.returncode==0 else None
    def image_ready(task_root):
        if not (task_root/'image-branch-intent.json').exists() or (task_root/'image-branch-result.json').exists() or (task_root/'start-intent.json').exists():return False
        result=image_status(task_root)
        return result is not None and result['state']=='running'
    def prepare_image(environment):
        image=None
        for action in ('save_environment','promote'):
            identity=str(uuid.uuid4());request={'op':'image-preview','id':identity,'environment':environment,'action':action}
            if image:request['image']=image
            preview=call(request);call(dict(request,op='image-submit',approve=preview['approval_sha256']))
            deadline=time.monotonic()+240
            while True:
                result=next(value for value in call(path='/view')['images'] if value['id']==identity)
                if result['state']=='succeeded':break
                assert result['state'] not in ('failed','cancelled') and time.monotonic()<deadline,result
                time.sleep(.1)
            result=result['receipt']['result'];image={'image':result['image'],'sha256':result['sha256']}
        (root/'image-reference.json').write_text(json.dumps(image));return image
    try:
        url=connect(previous_url)
        if args.reuse_service:
            environments=list((service/'application/environments').iterdir())
            assert len(environments)==1,'Select a service with one reviewed environment'
            installed={'id':environments[0].name}
        else:
            upload=call(path='/upload',raw=args.bundle.read_bytes())['upload']
            preview=call({'op':'environment-preview','upload':upload})
            installed=call({'op':'environment-import','upload':upload,'approve':preview['approval_sha256']})
        source=root/'source.pdf';make_pdf(source,'document_delivery.table_ambiguous' if args.clarification else 'document_delivery.table_revision' if args.source_revision else 'document_delivery.table_exact')
        upload=call(path='/upload',raw=source.read_bytes())['upload']
        task=str(uuid.uuid4())
        request={'op':'delivery-preview','id':task,'environment':installed['id'],'upload':upload,'instruction':'Preserve all source facts and deliver Markdown and Excel.'}
        if args.resume_task:request['resume_task']=args.resume_task
        if args.image_reference:request['image']=json.loads(args.image_reference.read_text())
        if args.prepare_image:request['image']=prepare_image(installed['id'])
        preview=call(request);creation=dict(request,op='delivery-create',approve=preview['approval_sha256'])
        assert call(creation)['id']==task
        assert call(creation)['id']==task
        blocked=dict(creation,id=str(uuid.uuid4()))
        try:call(blocked)
        except AssertionError as error:assert 'active document task' in str(error)
        else:raise AssertionError('Concurrent product task was not refused')
        deadline=time.monotonic()+660;answered=False;cancelled=False;restarted=False;lost=False
        original_jobs=None;loss_started=None;recovered_seconds=None
        task_root=service/'application/tasks'/task
        while time.monotonic()<deadline:
            state=call({'op':'delivery-view','id':task})['state']
            claims=service/'application/cocoon-vm-claims'
            setup_ready=claims.exists() and any(claim_belongs(path,task_root) for path in claims.glob('*.json')) and not (task_root/'intents/primary.json').exists()
            validator_ready=(task_root/'intents/intermediate-result.json').exists()
            branch_ready=image_ready(task_root) if args.lose_controller_at=='image-branch' or args.cancel_when_image_branch else False
            loss_ready=branch_ready if args.lose_controller_at=='image-branch' else setup_ready if args.lose_controller_at=='setup' else validator_ready if args.lose_controller_at=='validator' else state['state']=='waiting-clarification'
            if args.lose_controller and loss_ready and not lost:
                owner=json.loads((service/'application/tasks'/task/'owner.json').read_text())
                os.kill(owner['pid'],signal.SIGKILL);lost=True;loss_started=time.monotonic()
                time.sleep(.2)
                state=call({'op':'delivery-view','id':task})['state']
                assert state['state']=='unknown',state
                original_jobs=sorted(path.name for path in (service/'jobs').iterdir() if path.name not in existing_jobs)
                if args.drop_execution_projection:
                    retained=json.loads((task_root/'state.json').read_text())
                    retained['session']=None;retained['steps']={}
                    (task_root/'state.json').write_text(json.dumps(retained))
                assert call(creation)['id']==task
                call({'op':'delivery-cancel','id':task});cancelled=True
            if lost:
                state=call({'op':'delivery-reconcile','id':task})['state']
                assert time.monotonic()-loss_started<=30,('Original recovery exceeded 30 seconds',state.get('reconciliation'))
            if args.cancel and args.cancel_when_image_branch and branch_ready and not cancelled:
                call({'op':'delivery-cancel','id':task});cancelled=True
            if args.cancel and args.cancel_preparing and state.get('vm') and not cancelled:
                call({'op':'delivery-cancel','id':task});cancelled=True
            if args.cancel and not args.lose_controller and state.get('session') and not cancelled and (not args.cancel_when_clarifying or state['state']=='waiting-clarification'):
                call({'op':'delivery-cancel','id':task});cancelled=True
            if args.restart_page and state.get('session') and not restarted:
                page.terminate();page.wait(timeout=15)
                page=subprocess.Popen([str(host),'app','serve',str(service)],stdout=log,stderr=log)
                url=connect(url);restarted=True
            if state['state']=='waiting-clarification' and not answered and not args.cancel:
                assert args.clarification
                answers={key:'EUR' if key=='currency' else 12 for key in state['questions']}
                value={'op':'delivery-clarify','id':task,'answers':answers}
                approve(value)
                if args.drop_clarification_receipt:
                    repeated=approve(value);assert repeated['replayed'] is False
                answered=True
            if state['state'] in ('succeeded','failed','cancelled') or (state['state']=='unknown' and not lost):break
            time.sleep(.05 if args.lose_controller_at=='setup' else .2)
        if lost:
            recovered_seconds=time.monotonic()-loss_started
            assert recovered_seconds<=30,('Original-owner recovery exceeded 30 seconds',recovered_seconds)
            assert sorted(path.name for path in (service/'jobs').iterdir() if path.name not in existing_jobs)==original_jobs,'Recovery enqueued new work'
            if args.lose_controller_at in ('setup','image-branch'):assert not original_jobs,'Preparation fault admitted a primary'
            else:
                jobs=[json.loads((service/'jobs'/identity/'request.json').read_text()) for identity in original_jobs]
                assert sum(job['kind']=='harness' for job in jobs)==1,'Primary execution duplicated'
                credential=json.loads((task_root/'credential.json').read_text())
                primary=next(job for job in jobs if job['kind']=='harness')
                late=root/'late-request.json';late.write_text(json.dumps({'credential':credential,'request':{'op':'harness','task':primary['task']}}))
                refused=subprocess.run([str(host),'daemon','task-request',str(service),str(late)],capture_output=True,text=True,timeout=15)
                assert refused.returncode and 'revoked' in refused.stderr,'Late original authorization remained executable'
        assert state['state']==('cancelled' if args.cancel else 'succeeded'),state
        products={}
        assert state['termination_confirmed']
        assert state['runtime_termination_confirmed']
        if args.lose_controller_at=='image-branch' or args.cancel_when_image_branch:
            assert image_status(task_root)['state']=='cancelled'
            assert not (task_root/'start-intent.json').exists()
            assert not state.get('session')
        if args.cancel:assert 'final' not in state.get('products',{})
        if not args.cancel:
            assert state['termination_confirmed']
            for stage,name in [('intermediate','accepted.md'),('final','result.xlsx')]:
                artifact=call({'op':'delivery-artifact','id':task,'stage':stage})
                output=root/name;output.write_bytes(bytes(artifact['bytes']));products[name]=hashlib.sha256(output.read_bytes()).hexdigest()
        if not args.cancel:
            from zipfile import ZipFile
            from xml.etree import ElementTree as ET
            from decimal import Decimal
            namespace={'s':'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}
            with ZipFile(root/'result.xlsx') as archive:
                sheet=ET.fromstring(archive.read('xl/worksheets/sheet1.xml'))
                cells={cell.attrib['r']:cell for cell in sheet.findall('.//s:sheetData/s:row/s:c',namespace)}
                assert Decimal(cells['D5'].findtext('s:v',namespaces=namespace))==Decimal('39.00' if args.source_revision and not args.clarification else '38.50')
                assert cells['D5'].findtext('s:f',namespaces=namespace)=='SUM(D2:D4)'
                for row in range(2,5):assert cells[f'D{row}'].findtext('s:f',namespaces=namespace)==f'B{row}*C{row}'
            markdown=(root/'accepted.md').read_text()
            for item in ('Bolt','Plate','Bearing'):assert item in markdown
        case_jobs=[json.loads((path/'request.json').read_text()) for path in (service/'jobs').iterdir() if path.name not in existing_jobs]
        primary_count=sum(job['kind']=='harness' for job in case_jobs)
        assert primary_count<=1,'Product task admitted more than one primary'
        if not args.cancel:assert primary_count==1 and len(case_jobs)==3,'Delivery bypassed original validators'
        assert hashlib.sha256(host.read_bytes()).hexdigest()==host_digest,'Selected host binary changed during acceptance'
        (root/'report.json').write_text(json.dumps({'passed':True,'real_model':bool(state.get('session')) and any((service/'jobs'/state['session']).glob('turn-result-*.json')),'task':task,'primary_count':primary_count,'native_validator_count':len(case_jobs)-primary_count,'primary_admitted':bool(state.get('session')),'empty_service':not bool(args.reuse_service),'product_http':True,
            'image_branch_cancelled':bool(args.cancel_when_image_branch or args.lose_controller_at=='image-branch'),'clarification':answered,'cancelled':cancelled,'page_restarted':restarted,'observation_reconnects':observation_reconnects,'controller_lost':lost,'loss_boundary':args.lose_controller_at or 'clarification','projection_dropped':args.drop_execution_projection,'recovery_seconds':recovered_seconds,'original_jobs':original_jobs,'clarification_receipt_retried':args.drop_clarification_receipt,'products':products,
            'resume_task':args.resume_task,'source_revision':args.source_revision,'host_sha256':hashlib.sha256(host.read_bytes()).hexdigest()},indent=2))
    finally:
        if task and base:
            try:
                state=call({'op':'delivery-view','id':task})['state']
                if state['state'] not in ('succeeded','failed','cancelled','unknown'):call({'op':'delivery-cancel','id':task})
            except Exception:pass
        if page.poll() is None:page.terminate();page.wait(timeout=15)
        marker=service/'endpoint.json'
        if marker.exists():
            pid=json.loads(marker.read_text()).get('pid')
            if pid:
                try:os.kill(pid,signal.SIGTERM)
                except ProcessLookupError:pass
                deadline=time.monotonic()+15
                while marker.exists() and time.monotonic()<deadline:time.sleep(.025)
                assert not marker.exists(),'Original daemon shutdown unconfirmed'
        log.close()


if __name__=='__main__':main()

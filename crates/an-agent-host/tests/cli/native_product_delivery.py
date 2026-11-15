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
from document_benchmark import make_pdf
from host_qualification import require_native_workspace


def claim_belongs(path,root):
    try:return json.loads(path.read_text()).get('session')==str(root)
    except (FileNotFoundError,json.JSONDecodeError):return False


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('host','bundle','evidence'):parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--reuse-service',type=Path)
    parser.add_argument('--clarification',action='store_true')
    parser.add_argument('--cancel',action='store_true')
    parser.add_argument('--cancel-preparing',action='store_true')
    parser.add_argument('--cancel-when-clarifying',action='store_true')
    parser.add_argument('--restart-page',action='store_true')
    parser.add_argument('--lose-controller',action='store_true')
    parser.add_argument('--lose-controller-at',choices=('setup','validator'))
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
    endpoint=service/'application/endpoint.json';task=None;base=None;token=None
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
        data=raw if raw is not None else json.dumps(value).encode() if value is not None else None
        request=urllib.request.Request(base+path,data=data,headers={'Origin':base,'Authorization':'Bearer '+token,'Content-Type':'application/json' if raw is None else 'application/octet-stream'})
        try:
            with urllib.request.urlopen(request,timeout=300) as response:return json.load(response)
        except urllib.error.HTTPError as error:
            raise AssertionError((path,error.code,error.read().decode())) from error
    def approve(value):
        preview=call(value);return call(dict(value,approve=preview['approval_sha256']))
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
        source=root/'source.pdf';make_pdf(source,'document_delivery.table_ambiguous' if args.clarification else 'document_delivery.table_exact')
        upload=call(path='/upload',raw=source.read_bytes())['upload']
        import uuid
        task=str(uuid.uuid4())
        request={'op':'delivery-preview','id':task,'environment':installed['id'],'upload':upload,'instruction':'Preserve all source facts and deliver Markdown and Excel.'}
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
            loss_ready=setup_ready if args.lose_controller_at=='setup' else validator_ready if args.lose_controller_at=='validator' else state['state']=='waiting-clarification'
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
            if args.lose_controller_at=='setup':assert not original_jobs,'Preparation fault admitted a primary'
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
                assert Decimal(cells['D5'].findtext('s:v',namespaces=namespace))==Decimal('38.50')
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
            'clarification':answered,'cancelled':cancelled,'page_restarted':restarted,'controller_lost':lost,'loss_boundary':args.lose_controller_at or 'clarification','projection_dropped':args.drop_execution_projection,'recovery_seconds':recovered_seconds,'original_jobs':original_jobs,'clarification_receipt_retried':args.drop_clarification_receipt,'products':products,
            'host_sha256':hashlib.sha256(host.read_bytes()).hexdigest()},indent=2))
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

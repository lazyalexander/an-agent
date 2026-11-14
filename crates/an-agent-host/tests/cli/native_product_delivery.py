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


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('host','bundle','evidence'):parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--clarification',action='store_true')
    parser.add_argument('--cancel',action='store_true')
    parser.add_argument('--cancel-preparing',action='store_true')
    parser.add_argument('--cancel-when-clarifying',action='store_true')
    parser.add_argument('--restart-page',action='store_true')
    parser.add_argument('--lose-controller',action='store_true')
    parser.add_argument('--drop-clarification-receipt',action='store_true')
    args=parser.parse_args();require_native_workspace()
    host=args.host.resolve(strict=True);host_digest=hashlib.sha256(host.read_bytes()).hexdigest();root=args.evidence.resolve();root.mkdir(mode=0o700)
    service=root/'service';log=(root/'page.log').open('w')
    page=subprocess.Popen([str(host),'app','serve',str(service)],stdout=log,stderr=log)
    endpoint=service/'application/endpoint.json';task=None;base=None;token=None
    def connect(previous=None):
        nonlocal base,token
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            if endpoint.exists():
                url=json.loads(endpoint.read_text())['url']
                if url!=previous:base,token=url.split('/#');return url
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
        url=connect()
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
        while time.monotonic()<deadline:
            state=call({'op':'delivery-view','id':task})['state']
            if args.lose_controller and state['state']=='waiting-clarification' and not lost:
                owner=json.loads((service/'application/tasks'/task/'owner.json').read_text())
                os.kill(owner['pid'],signal.SIGKILL);lost=True
                time.sleep(.2)
                state=call({'op':'delivery-view','id':task})['state']
                assert state['state']=='unknown',state
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
            time.sleep(.2)
        assert state['state']==('cancelled' if args.cancel else 'succeeded'),state
        products={}
        assert state['termination_confirmed']
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
        assert hashlib.sha256(host.read_bytes()).hexdigest()==host_digest,'Selected host binary changed during acceptance'
        (root/'report.json').write_text(json.dumps({'passed':True,'real_model':True,'empty_service':True,'product_http':True,
            'clarification':answered,'cancelled':cancelled,'page_restarted':restarted,'controller_lost':lost,'clarification_receipt_retried':args.drop_clarification_receipt,'products':products,
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
        log.close()


if __name__=='__main__':main()

#!/usr/bin/env python3
"""One original primary session; fixed guest programs and original native custody."""
import argparse
import hashlib
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import sysconfig
import time
import uuid
from app_server_support import Driver, sha
from document_benchmark import make_pdf
from host_qualification import require_native_workspace


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('host','profile','domain','python-runtime','evidence'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--dependency-site',type=Path,required=True)
    parser.add_argument('--codex',type=Path)
    parser.add_argument('--model',default='gpt-6.1-sol')
    parser.add_argument('--clarification',action='store_true')
    parser.add_argument('--lost-receipt',action='store_true')
    parser.add_argument('--cancel-before-continuation',action='store_true')
    parser.add_argument('--cancel-during-program',action='store_true')
    parser.add_argument('--restart-controller',action='store_true')
    args=parser.parse_args();require_native_workspace()
    domain=args.domain.resolve(strict=True)
    sys.path.insert(0,str(args.dependency_site.resolve(strict=True)))
    sys.path.insert(0,str(domain/'silk'))
    sys.dont_write_bytecode=True
    import document
    runtime=args.python_runtime.resolve(strict=True)
    driver=Driver(args.host,args.evidence);driver.expires=int(time.time()*1000)+900000
    try:
        source=driver.root/'source.pdf';make_pdf(source,'document_delivery.table_ambiguous' if args.clarification else 'document_delivery.table_exact')
        accepted_md=driver.root/'accepted.md';accepted_md.touch()
        contract=domain/'knowledge/contract.md'
        program=domain/'silk/document.py'
        if args.cancel_during_program:
            program=driver.root/'slow-program.py';program.write_text('import time\ntime.sleep(30)\n')
        transport=args.profile.resolve(strict=True)
        specs={};operations={name:str(uuid.uuid4()) for name in ('extract','markdown','workbook')}
        output_specs={'extract':{'source.json':'utf-8'},'markdown':{'markdown.md':'utf-8'},'workbook':{'result.xlsx':'bytes','verification.json':'utf-8'}}
        for action in operations:
            path=driver.root/(action+'-spec.json')
            path.write_text(json.dumps({'transport':str(transport),'transport_sha256':sha(transport),
                'program':str(program),'program_sha256':sha(program),'interpreter':'/opt/python/bin/python3.12',
                'arguments':['-I','-B','{program}','job-'+action,'{request}','{output}'],'outputs':output_specs[action]}))
            specs[action]=path
        def job(action,data,inputs):
            return {'op':'cocoon-job','spec':str(specs[action]),'spec_sha256':sha(specs[action]),
                    'operation_id':operations[action],'data':data,'inputs':inputs}
        answers={'currency':'EUR','quantity:0':12}
        answers_file=driver.root/'clarifications.json';answers_file.write_text(json.dumps(answers))
        answers_ref={'op':'read-input','path':str(answers_file),'sha256':sha(answers_file)}
        source_ref={'op':'read-input','path':str(source),'sha256':sha(source)}
        knowledge={'op':'read-input','path':str(contract),'sha256':sha(contract),'encoding':'utf-8'}
        binary=args.codex.resolve(strict=True) if args.codex else driver.root/'protocol-fixture'
        fixed_md='| Item | Quantity | Unit price (EUR) |\n| --- | --- | --- |\n| Bolt | 12 | 0.50 |\n| Plate | 3 | 8.00 |\n| Bearing | 2 | 4.25 |\n'
        if not args.codex:
            binary.write_text('#!'+sys.executable+'\n'+'''import json,sys
if '--version' in sys.argv: print('codex-cli 0.166.0');sys.exit()
def send(v): print(json.dumps(v),flush=True)
count=0; pending=[]; success=True
for raw in sys.stdin:
 v=json.loads(raw)
 if v.get('method')=='initialize':send({'id':v['id'],'result':{}});continue
 elif v.get('method')=='thread/start':send({'id':v['id'],'result':{'thread':{'id':'document-primary'}}});continue
 elif v.get('method')=='turn/start':
  count+=1;turn='turn-'+str(count);success=True
  send({'id':v['id'],'result':{'turn':{'id':turn}}})
  send({'method':'turn/started','params':{'threadId':'document-primary','turn':{'id':turn}}})
  prompt=v['params']['input'][0]['text'];pending=json.loads(prompt[prompt.index('REQUESTS\\n')+9:])
 elif v.get('id')==99:
  success=success and v['result']['success']
  if not success: print(json.dumps(v),file=sys.stderr)
 else:continue
 if pending:
  send({'id':99,'method':'item/tool/call','params':{'threadId':'document-primary','turnId':turn,'tool':'host_request',
   'namespace':None,'callId':str(len(pending)),'arguments':{'request':json.dumps(pending.pop(0))}}})
 else:
  proposal={'ok':success}
  if CLARIFICATION: proposal={'ok':False,'questions':['currency','quantity:0']} if count==1 else {'ok':success,'questions':[]}
  send({'method':'item/completed','params':{'threadId':'document-primary','turnId':turn,'item':{
   'type':'agentMessage','phase':'final_answer','text':json.dumps(proposal)}}})
  send({'method':'turn/completed','params':{'threadId':'document-primary','turn':{'id':turn,'status':'completed'}}})
'''.replace('CLARIFICATION','True' if args.clarification else 'False'));binary.chmod(0o700)
        requests=[knowledge,job('extract',{},[source_ref]),job('markdown',{'markdown':'' if args.codex else fixed_md},[source_ref])]
        prompt=('Read the domain contract through host_request. Execute the fixed extract job below and read its returned source.json text. '
            'Preserve every source fact and produce the required Markdown table. Then execute the fixed markdown job using your Markdown '
            'in data.markdown (replace the sample Markdown if needed). Use the exact job IDs and authorized input references supplied. '
            'No shell, filesystem or other tools are available. Return {"ok":true} only if both fixed jobs report state=proposed. '
            'The host will accept Markdown before the next turn. Do not create model subagents.\nREQUESTS\n'+json.dumps(requests))
        if args.clarification:
            requests=[knowledge,job('extract',{},[source_ref])]
            prompt=('Read the domain contract, then execute the extract job. If requires_clarification is nonempty, do not guess or '
                'call the Markdown job. Return {"ok":false,"questions":[exact keys from requires_clarification]}. '
                'The user will supply approved answers in the next turn.\nREQUESTS\n'+json.dumps(requests))
        task=dict(driver.task(binary,prompt),turn_limit=3 if args.clarification else 2,wall_seconds=600)
        if args.clarification:
            task['schema']={'type':'object','properties':{'ok':{'type':'boolean'},'questions':{'type':'array','items':{'type':'string'}}},
                'required':['ok','questions'],'additionalProperties':False}
        if args.codex:task['model']=args.model
        instances={}
        # Fixed native release bodies share the original supervisor and publication path.
        for name in ('markdown','workbook'):
            package=driver.root/('package-'+name);(package/'silk').mkdir(parents=True)
            if name=='markdown':entry='silk/main.sh';content=b'set -eu\ncat "$AN_SPOOL_INPUT" > markdown.md\n';outputs=['markdown.md'];binary_outputs=[];kind='macos-native-process-v1'
            else:entry='silk/document.py';content=program.read_bytes();outputs=['result.xlsx','verification.json'];binary_outputs=['result.xlsx'];kind='macos-native-python-v1'
            (package/entry).write_bytes(content)
            (package/'native-spool.json').write_text(json.dumps({'schema':'an-native-spool-v3','name':'document-'+name,'version':'0.1.0',
                'events':{'consumes':[],'produces':['artifact.accepted']},'entry':entry,'files':{entry:hashlib.sha256(content).hexdigest()},
                'outputs':outputs,'binary_outputs':binary_outputs,'requirements':{'runtime':kind,'os':'macos','arch':'portable-source',
                'interaction':'headless','gpu':'none','inputs':['input'],'editable':[],'dependencies':[],'capabilities':['read-input','write-declared-outputs']}}))
            instance=driver.root/('instance-'+name)
            if name=='workbook':
                site=driver.root/'empty-site';site.mkdir()
                stdlib=Path(subprocess.check_output([str(runtime),'-S','-c',"import sysconfig;print(sysconfig.get_path('stdlib'))"],text=True).strip()).resolve()
                profile=driver.root/'native-python.json';profile.write_text(json.dumps({'schema':'an-native-python-runtime-v1','executable':str(runtime),'executable_sha256':sha(runtime),
                    'stdlib':str(stdlib),'site':str(site),'site_sha256':hashlib.sha256(b'{}').hexdigest(),'dependencies':[],
                    'readonly':[str(runtime.parent.parent)],'inputs':[str(accepted_md)]}))
                driver.call('spool-load',package,instance,'--runtime',profile)
            else:driver.call('spool-load',package,instance)
            driver.request({'op':'register','instance':str(instance)});instances[name]=instance
        rights=[driver.profile(task),{'kind':'read-input','path':str(source),'sha256':sha(source)},
                {'kind':'read-input','path':str(contract),'sha256':sha(contract)}]
        if args.clarification:rights.append({'kind':'read-input','path':str(answers_file),'sha256':sha(answers_file)})
        rights += [{'kind':'cocoon-job','spec_sha256':sha(path)} for path in specs.values()]
        for name,output in [('markdown','markdown.md'),('workbook','result.xlsx')]:
            resource={'instance':str(instances[name]),'run':'deliver'}
            rights += [{'kind':'run-job-output',**resource,'operation':operations[name],'output':output,'seconds':30},
                       {'kind':'accept',**resource},{'kind':'artifact',**resource,'output':output}]
        credential=driver.grant(rights,delegate=True)
        session=driver.approve({'op':'harness','task':task},credential)['id']
        def wait_turn(number,expected_ok=True):
            deadline=time.monotonic()+600;marker=driver.service/'jobs'/session/f'turn-result-{number}.json'
            while not marker.exists():
                if time.monotonic()>deadline:raise AssertionError('original turn timed out; inspect without replay')
                view=driver.request({'op':'task','id':session},credential)
                if view['state']=='settled':raise AssertionError(('original session ended',view))
                time.sleep(.1)
            result=driver.request({'op':'harness-turn-result','id':session,'turn':number},credential)
            assert result['proposal']['ok'] is expected_ok,result
            return result['proposal']
        if args.cancel_during_program:
            directory=driver.service/'jobs'/session
            key=hashlib.sha256((operations['extract']+':execute').encode()).hexdigest()[:32]
            marker=directory/'cocoon-jobs'/operations['extract']/'operations'/(key+'.json')
            deadline=time.monotonic()+60
            while not marker.exists():
                assert time.monotonic()<deadline
                time.sleep(.01)
            time.sleep(.1)
            started=time.monotonic();view=driver.request({'op':'harness-connect','id':session},credential)
            observation_ms=(time.monotonic()-started)*1000
            assert observation_ms<1500,observation_ms
            started=time.monotonic()
            driver.approve({'op':'harness-steer','id':session,'execution':view['execution'],
                'command':'during-program','instruction':'Preserve all input facts; do not add effects.'},credential)
            steering_ms=(time.monotonic()-started)*1000
            assert steering_ms<1500,steering_ms
            started=time.monotonic();driver.approve({'op':'cancel','id':session},credential)
            cancellation_ms=(time.monotonic()-started)*1000
            assert cancellation_ms<1500,cancellation_ms
            driver.wait(session,credential,'cancelled')
            result=json.loads((directory/'completion.json').read_text())['result']
            assert result['termination_confirmed'] and result['cocoon']['termination_confirmed']
            assert not (directory/'cocoon-jobs'/operations['extract']/'result.json').exists()
            (driver.root/'report.json').write_text(json.dumps({'passed':True,'cancel_during_program':True,
                'real_model':bool(args.codex),'control_observation_ms':observation_ms,'steering_ms':steering_ms,
                'cancellation_admission_ms':cancellation_ms,'vm_termination_confirmed':True},indent=2))
            return
        first=wait_turn(1,not args.clarification)
        if args.cancel_before_continuation:
            driver.approve({'op':'cancel','id':session},credential)
            driver.wait(session,credential,'cancelled')
            completion=json.loads((driver.service/'jobs'/session/'completion.json').read_text())['result']
            assert completion['termination_confirmed'] and completion['cocoon']['termination_confirmed']
            retained=driver.request({'op':'cocoon-job-status','id':session,'operation_id':operations['extract']},credential)
            assert retained['state']=='proposed'
            driver.request({'op':'submit','instance':str(instances['markdown']),'run':'deliver','seconds':30,
                'job_output':{'session':session,'operation':operations['markdown'],'output':'markdown.md'}},credential,ok=False)
            (driver.root/'report.json').write_text(json.dumps({'passed':True,'real_model':bool(args.codex),
                'cancelled':True,'single_original_session':True,'vm_termination_confirmed':True,'no_late_acceptance':True},indent=2))
            return
        markdown_turn=1
        if args.clarification:
            assert set(first['questions'])==set(answers),first
            before=driver.request({'op':'harness-connect','id':session},credential)
            supplement=[answers_ref,job('markdown',{'markdown':'' if args.codex else fixed_md},[source_ref,answers_ref])]
            instruction=('The user confirms EUR and Bolt quantity 12. Read the approved clarification input. Then propose Markdown preserving '
                'all source facts, and execute the fixed markdown job with your Markdown in data.markdown. Keep both authorized inputs '
                'in its inputs list. Return {"ok":true,"questions":[]} only when the job is proposed.\nREQUESTS\n'+json.dumps(supplement))
            driver.approve({'op':'harness-continue','id':session,'execution':before['execution'],'after_turn':1,
                'command':'user-clarification','instruction':instruction},credential)
            wait_turn(2);markdown_turn=2
        job_root=driver.service/'jobs'/session/'cocoon-jobs'
        md_result=json.loads((job_root/operations['markdown']/'result.json').read_text())
        md=md_result['outputs']['markdown.md']['text']
        document.validate_markdown(document.extract_pdf(source),md,answers if args.clarification else None)
        def accept(name,output):
            resource={'instance':str(instances[name]),'run':'deliver'}
            submitted=driver.approve({'op':'submit',**resource,'seconds':30,
                'job_output':{'session':session,'operation':operations[name],'output':output}},credential)['id']
            driver.wait(submitted,credential)
            driver.approve({'op':'accept',**resource},credential)
            result=driver.request({'op':'artifact-read',**resource,'output':output},credential)
            return result
        accepted=accept('markdown','markdown.md');accepted_md.write_bytes(bytes(accepted['bytes']))
        assert accepted_md.read_text()==md
        view=driver.request({'op':'harness-connect','id':session},credential)
        owner=(driver.service/'jobs'/session/'owner.json').read_bytes()
        if args.restart_controller:
            driver.process.send_signal(signal.SIGTERM);driver.process.wait(timeout=15)
            driver.process=subprocess.Popen([str(driver.host),'daemon','serve',str(driver.service)],stdout=driver.log,stderr=driver.log)
            deadline=time.monotonic()+15
            while not (driver.service/'endpoint.json').exists():
                assert driver.process.poll() is None and time.monotonic()<deadline
                time.sleep(.05)
            reconnected=driver.request({'op':'harness-connect','id':session,'cursor':view['cursor']},credential)
            assert reconnected['execution']==view['execution']
            assert (driver.service/'jobs'/session/'owner.json').read_bytes()==owner
            assert len(reconnected['cocoon_jobs'])>=2
        read_md={'op':'artifact-read','instance':str(instances['markdown']),'run':'deliver','output':'markdown.md'}
        proposal={'source_sha256':'' if args.codex else sha(accepted_md),'rows':[] if args.codex else document.parse_markdown(md)}
        second=[read_md,job('workbook',proposal,[read_md])]
        recovery_file=None
        if args.lost_receipt:
            md_root=job_root/operations['markdown']
            key=hashlib.sha256((operations['markdown']+':execute').encode()).hexdigest()[:32]
            recovery_file=md_root/'operations'/(key+'.result.json')
            original_intent=(md_root/'operations'/(key+'.json')).read_bytes()
            assert recovery_file.exists()
            recovery_file.unlink();(md_root/'result.json').unlink()
            uncertain=driver.request({'op':'cocoon-job-status','id':session,'operation_id':operations['markdown']},credential)
            assert uncertain['state']=='running-or-unknown'
            recovery=job('markdown',{'markdown':md},[source_ref,answers_ref] if args.clarification else [source_ref])
            second.insert(0,recovery)
        instruction=('Read the accepted Markdown using the supplied artifact reference. Extract its exact rows and use its content_sha256 '
            'as source_sha256. Execute the fixed workbook job using your structured proposal in data. Its input must remain the supplied '
            'artifact reference. Return {"ok":true} only when state=proposed and verification.json reports passed=true. '
            'If a recovery request precedes the artifact read, submit its exact original data and ID once to reconcile the missing receipt; do not allocate a new ID. Use the original primary session only.\nREQUESTS\n'+json.dumps(second))
        driver.approve({'op':'harness-continue','id':session,'execution':view['execution'],'after_turn':markdown_turn,
            'command':'accepted-markdown','instruction':instruction},credential)
        wait_turn(markdown_turn+1);driver.wait(session,credential)
        if args.lost_receipt:
            assert recovery_file.exists()
            assert (md_root/'operations'/(key+'.json')).read_bytes()==original_intent
            assert json.loads((md_root/'result.json').read_text())==md_result
        accepted=accept('workbook','result.xlsx')
        delivery=driver.root/'result.xlsx';delivery.write_bytes(bytes(accepted['bytes']))
        report=document.verify_workbook(md,delivery)
        assert report['total_eur']=='38.50'
        after=driver.request({'op':'harness-connect','id':session},credential)
        assert after['execution']==view['execution'] and (driver.service/'jobs'/session/'owner.json').read_bytes()==owner
        completion=json.loads((driver.service/'jobs'/session/'completion.json').read_text())['result']
        assert completion['termination_confirmed'] and completion['cocoon']['termination_confirmed']
        activity=driver.request({'op':'activity','id':session},credential)
        summary={'passed':True,'model_calls':None if args.codex else 0,'model_turns':(3 if args.clarification else 2) if args.codex else 0,'real_model':bool(args.codex),'protocol_fixture':not bool(args.codex),
            'single_original_session':True,'guest_document_programs':3,'clarification':args.clarification,'lost_receipt':args.lost_receipt,'controller_restarted':args.restart_controller,'host_custody':True,'total_eur':'38.50',
            'vm_termination_confirmed':True,'workbook_sha256':sha(delivery),'host_sha256':sha(driver.host),
            'domain_program_sha256':sha(program),'metrics':activity.get('harness',{}).get('metrics')}
        (driver.root/'report.json').write_text(json.dumps(summary,indent=2))
    finally:driver.close()


if __name__=='__main__':main()

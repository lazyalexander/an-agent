"""Trusted host coordination over original supervisors; no provider protocol loop."""
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid


def read(path):
    return json.loads(Path(path).read_text())


def sha(path):
    value=hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda:stream.read(65536),b''):value.update(block)
    return value.hexdigest()


def save(path,value):
    path=Path(path);temporary=path.with_name('.'+path.name+'.'+uuid.uuid4().hex)
    with temporary.open('x') as stream:
        os.chmod(temporary,0o600);json.dump(value,stream);stream.flush();os.fsync(stream.fileno())
    os.replace(temporary,path)
    descriptor=os.open(path.parent,os.O_RDONLY)
    try:os.fsync(descriptor)
    finally:os.close(descriptor)


class Client:
    def __init__(self,host,service,root):
        self.host,self.service,self.root=host,service,root

    def cli(self,*args):
        result=subprocess.run([self.host,*map(str,args)],capture_output=True,text=True,timeout=40)
        if result.returncode:raise RuntimeError(result.stderr[:2000])
        return json.loads(result.stdout)

    def request(self,value,credential=None):
        path=self.root/('request-'+uuid.uuid4().hex+'.json')
        save(path,{'credential':credential,'request':value} if credential else value)
        try:return self.cli('daemon','task-request' if credential else 'request',self.service,path)
        finally:path.unlink()

    def approve(self,value,credential=None,record=None):
        preview=self.request(value,credential)
        if record:
            identity=preview.get('plan',{}).get('task_sha256') if value['op']=='harness' else preview['approval_sha256']
            if not identity:raise ValueError('Original request identity unavailable')
            journal=self.root/'intents';journal.mkdir(exist_ok=True,mode=0o700)
            descriptor=os.open(self.root,os.O_RDONLY)
            try:os.fsync(descriptor)
            finally:os.close(descriptor)
            intent={'kind':'harness' if value['op']=='harness' else 'spool','request':identity,
                    'grant':credential['id'],'instance':value.get('instance'),'run':value.get('run')}
            path=journal/(record+'.json')
            if path.exists():raise RuntimeError('Execution intent already exists; reconcile original work')
            save(path,intent)
        result=self.request(dict(value,approve=preview['approval_sha256']),credential)
        if record:save(journal/(record+'-result.json'),result)
        return result


def unprojected_execution(root,state):
    return any((root/'intents'/(name+'.json')).exists() and not
               (state.get('session') if name=='primary' else state.get('steps',{}).get(name))
               for name in ('primary','intermediate','final'))


def main():
    host,service,root= sys.argv[1:]
    root=Path(root);service=Path(service)
    save(root/'recovery-version.json',{'version':1})
    config=read(root/'plan.json');environment=Path(config['environment'])
    manifest=read(environment/'runtime.json');components=manifest['components']
    recipe=read(environment/components['delivery'])
    domain=(environment/components['delivery']).parent
    source=root/'input';answers=root/'answers.json';accepted=root/'accepted-intermediate'
    save(answers,{})
    accepted.touch(exist_ok=False)
    client=Client(host,service,root);deadline=time.monotonic()+600
    state={'state':'preparing','products':{},'session':None,'steps':{},'runtime_cleanup_required':True}
    vm=None;api_process=None;credential=None;transport=None;work=None;vm_bound=False;setup_bound=False;cocoon_state=root/'cocoon'
    stopping=False;owners_confirmed=True;requested_outcome='failed'
    def stop(_signal,_frame):
        nonlocal stopping
        stopping=True
    signal.signal(signal.SIGTERM,stop);signal.signal(signal.SIGINT,stop)
    def check():
        if stopping or (root/'cancel.json').exists():raise InterruptedError('Task cancelled')
        if time.monotonic()>deadline:raise TimeoutError('Task deadline reached')
    def update(**values):
        state.update(values);save(root/'state.json',state)
    def direct(*args):
        result=subprocess.run([str(environment/components['cocoon']),'host',str(cocoon_state),*map(str,args)],capture_output=True,text=True,timeout=180)
        if result.returncode:raise RuntimeError('Cocoon control failed; inspect private records')
        return json.loads(result.stdout)
    def api(operation,grant='',request_id=None):
        path=root/'cocoon-request.json'
        save(path,{'request':request_id or str(uuid.uuid4()),'caller':'product-task','credential':'','grant':grant,'operation':operation})
        result=direct('api',path)['result']
        if 'Ok' not in result:raise RuntimeError('Cocoon refused operation')
        return result['Ok']
    def wait_job(job):
        while True:
            check();value=client.request({'op':'task','id':job},credential)
            if value['state']=='settled':
                if value['outcome']!='succeeded':raise RuntimeError('Original task did not succeed')
                return value
            time.sleep(.1)
    def wait_turn(number):
        while True:
            check();directory=service/'jobs'/state['session']
            if (directory/f'turn-result-{number}.json').exists():
                return client.request({'op':'harness-turn-result','id':state['session'],'turn':number},credential)['proposal']
            value=client.request({'op':'task','id':state['session']},credential)
            if value['state']=='settled':raise RuntimeError('Original primary session ended before delivery')
            time.sleep(.1)
    try:
        check()
        image=direct('image','import',environment/components['kernel'],environment/components['initrd'],environment/components['root_image'],'Reviewed offline runtime')['image']
        vm=str(uuid.uuid4());save(root/'setup.json',{'vm':vm,'image':image,'owner':'product-task'})
        update(vm=vm)
        direct('vm','create',vm,image,'product-task','retain')
        log=(root/'cocoon.log').open('w')
        save(root/'runtime-launch.json',{'automatic_retries':0})
        api_process=subprocess.Popen([str(environment/components['cocoon']),'host',str(cocoon_state),'serve',str(environment/components['runner'])],stdout=log,stderr=log)
        client.cli('app','task-runtime-owner',service,root.name,api_process.pid)
        socket=Path('/tmp')/('cocoon-host-'+hashlib.sha256(os.fsencode(cocoon_state)).hexdigest()+'.sock')
        for _ in range(300):
            check()
            if socket.exists():break
            if api_process.poll() is not None:raise RuntimeError('Cocoon service stopped')
            time.sleep(.05)
        check()
        start_id=str(uuid.uuid4());save(root/'start-intent.json',{'request':start_id,'vm':vm})
        active=api({'operation':'start','vm':vm},request_id=start_id)
        save(root/'start-result.json',active)
        work=api({'operation':'issue_grant','subject':'product-task','vm':vm,'actions':['inspect','guest','export','stop'],
                  'expires_unix':int(time.time())+900,'max_export_bytes':4194304})
        transport=root/'transport.json';save(transport,{'socket':str(socket),'grant':work['token'],'caller':'product-task','vm':vm,'generation':active['generation']})
        specs={};operations={name:str(uuid.uuid4()) for name in ('extract','intermediate','final')}
        for name in operations:
            spec=root/(name+'-spec.json')
            save(spec,{'transport':str(transport),'transport_sha256':sha(transport),'program':str(domain/recipe['program']),
                'program_sha256':sha(domain/recipe['program']),'interpreter':recipe['guest_interpreter'],
                'arguments':['-I','-B','{program}',recipe[name]['action'],'{request}','{output}'],'outputs':recipe[name]['outputs']})
            specs[name]=spec
        client.cli('app','task-bind-setup',service,root.name)
        setup_bound=True
        source_ref={'op':'read-input','path':str(source),'sha256':sha(source)}
        answer_ref=lambda:{'op':'read-input','path':str(answers),'sha256':sha(answers)}
        def job(name,inputs):
            return {'op':'cocoon-job','spec':str(specs[name]),'spec_sha256':sha(specs[name]),'operation_id':operations[name],'data':{},'inputs':inputs}
        knowledge=domain/recipe['knowledge']
        def prompt(key,requests):return recipe[key]+'\nUser task: '+config['instruction']+'\nREQUESTS\n'+json.dumps(requests)
        task={'app_server':True,'label':recipe['label'],'binary':config['codex'],'binary_sha256':config['codex_sha256'],
              'prompt':prompt('initial_instruction',[{'op':'read-input','path':str(knowledge),'sha256':sha(knowledge),'encoding':'utf-8'},job('extract',[source_ref]),job('intermediate',[source_ref])]),
              'schema':{'type':'object','properties':{'ok':{'type':'boolean'},'questions':{'type':'array','items':{'type':'string'}}},'required':['ok','questions'],'additionalProperties':False},
              'scope':{'single_primary':True,'cocoon_job_ids':{sha(specs[name]):operations[name] for name in specs}},'model':config['model'],'mcp':None,'turn_limit':3,'wall_seconds':600}
        instance_root=service.parent/('an-product-'+hashlib.sha256(os.fsencode(service)).hexdigest()[:16])/root.name
        instance_root.mkdir(parents=True,mode=0o700)
        instances={}
        for name in ('intermediate','final'):
            check()
            package=environment/components[name+'_package'];package=package.parent
            instance=instance_root/(name+'-instance');profile=root/(name+'-runtime.json')
            site=environment/components['native_site'];site=site.parent
            if not recipe[name]['dependencies']:
                site=root/'empty-site';site.mkdir()
            inventory={str(p.relative_to(site)):sha(p) for p in sorted(site.rglob('*')) if p.is_file()}
            native=environment/components['native_python'];stdlib=(environment/components['native_stdlib']).parent
            save(profile,{'schema':'an-native-python-runtime-v1','executable':str(native),'executable_sha256':sha(native),
                 'stdlib':str(stdlib),'site':str(site),'site_sha256':hashlib.sha256(json.dumps(inventory,separators=(',',':'),sort_keys=True).encode()).hexdigest(),
                 'dependencies':recipe[name]['dependencies'],'readonly':[str(native.parent.parent)],
                 'inputs':[str(source),str(answers)] if name=='intermediate' else [str(accepted)]})
            client.cli('spool-load',package,instance,'--runtime',profile)
            client.request({'op':'register','instance':str(instance)});instances[name]=instance
        update(instances={name:str(path) for name,path in instances.items()})
        rights=[client.request({'op':'harness-permission','task':task}),{'kind':'read-input','path':str(source),'sha256':sha(source)},
                {'kind':'read-input','path':str(knowledge),'sha256':sha(knowledge)}]
        rights += [{'kind':'cocoon-job','spec_sha256':sha(spec)} for spec in specs.values()]
        # Answers are immutable once supplied; derive their exact read capability then.
        rights.append({'kind':'read-input','path':str(answers),'sha256':sha(answers)})
        for name in instances:
            resource={'instance':str(instances[name]),'run':'deliver'}
            rights += [{'kind':'run-job-output',**resource,'operation':operations[name],'output':recipe[name]['output'],'seconds':30},
                       {'kind':'accept',**resource},{'kind':'artifact',**resource,'output':recipe[name]['output']}]
        scope={'task':root.name,'subject':'primary','expires_ms':int(time.time()*1000)+900000,'delegate':True,'permissions':rights}
        credential=client.approve({'op':'grant','scope':scope})['credential'];save(root/'credential.json',credential)
        check()
        session=client.approve({'op':'harness','task':task},credential,record='primary')['id']
        state['session']=session
        client.approve({'op':'harness-bind-cocoon','id':session,'spec':str(specs['extract']),'spec_sha256':sha(specs['extract'])})
        vm_bound=True;update(state='running',session=session,operations=operations)
        first=wait_turn(1);turn=1
        if not first['ok']:
            contract=recipe['clarification_source']
            extraction=read(service/'jobs'/session/'cocoon-jobs'/operations['extract']/'result.json')
            facts=json.loads(extraction['outputs'][contract['output']]['text'])
            if set(first['questions'])!=set(facts[contract['keys']]):raise ValueError('Clarification changed extracted requirements')
            fields=[]
            for key in first['questions']:
                family,_,index=key.partition(':')
                field=dict(recipe['clarifications'][family],key=key)
                if index:field['source_label']=facts[contract['rows']][int(index)][contract['label']]
                fields.append(field)
            update(state='waiting-clarification',questions=first['questions'],clarifications=fields)
            # A new immutable answer file requires a separately approved exact grant.
            while not (root/'clarification.json').exists():check();time.sleep(.1)
            response=read(root/'clarification.json')
            supplied=response['answers'];expected=set(first['questions'])
            if set(supplied)!=expected:raise ValueError('Clarification keys changed')
            save(answers,supplied)
            extra=client.approve({'op':'grant','scope':{'task':scope['task'],'subject':'user-answers',
                'expires_ms':scope['expires_ms'],'delegate':False,'permissions':[{'kind':'read-input','path':str(answers),'sha256':sha(answers)}]}})['credential']
            client.approve({'op':'harness-attach-input','id':session,'input_grant':extra['id']})
            view=client.request({'op':'harness-connect','id':session},credential)
            client.approve({'op':'harness-continue','id':session,'execution':view['execution'],'after_turn':1,
                'command':'user-clarification','instruction':prompt('clarification_instruction',[answer_ref(),job('intermediate',[source_ref,answer_ref()])])},credential)
            update(state='running');turn=2
            if not wait_turn(turn)['ok']:raise RuntimeError('Clarified proposal failed')
        def accept(name):
            check()
            resource={'instance':str(instances[name]),'run':'deliver'}
            pending=client.approve({'op':'submit',**resource,'seconds':30,'job_output':{'session':session,'operation':operations[name],'output':recipe[name]['output']}},credential,record=name)['id']
            state['steps'][name]=pending;update(state='validating')
            wait_job(pending);check();client.approve({'op':'accept',**resource},credential)
            artifact=client.request({'op':'artifact-read',**resource,'output':recipe[name]['output']},credential)
            state['products'][name]={**resource,'output':recipe[name]['output']};update()
            return bytes(artifact['bytes'])
        accepted.write_bytes(accept('intermediate'))
        reference={'op':'artifact-read',**state['products']['intermediate']}
        view=client.request({'op':'harness-connect','id':session},credential)
        client.approve({'op':'harness-continue','id':session,'execution':view['execution'],'after_turn':turn,
            'command':'accepted-intermediate','instruction':prompt('final_instruction',[reference,job('final',[reference])])},credential)
        update(state='running');wait_turn(turn+1);accept('final')
        if turn+1<task['turn_limit']:
            client.approve({'op':'harness-finish','id':session,'execution':view['execution'],'after_turn':turn+1},credential)
        wait_job(session)
        completion=read(service/'jobs'/session/'completion.json')['result']
        if not completion['termination_confirmed'] or not completion.get('cocoon',{}).get('termination_confirmed'):raise RuntimeError('Original release is unconfirmed')
        requested_outcome='succeeded';update(state='stopping')
    except Exception as error:
        requested_outcome='cancelled' if isinstance(error,InterruptedError) else 'failed'
        update(state='stopping',error=str(error))
        if unprojected_execution(root,state):
            owners_confirmed=False
            update(state='unknown',error='Original request may be admitted; reconcile without replay')
        for pending in state['steps'].values():
            try:client.approve({'op':'cancel','id':pending},credential)
            except Exception:state['state']='unknown';save(root/'state.json',state)
        if state['session']:
            try:client.approve({'op':'cancel','id':state['session']},credential)
            except Exception:state['state']='unknown';save(root/'state.json',state)
        targets=list(state['steps'].values())+([state['session']] if state['session'] else [])
        release_deadline=time.monotonic()+30
        for target in targets:
            confirmed=False
            while time.monotonic()<release_deadline:
                try:
                    observed=client.request({'op':'task','id':target},credential)
                    if observed['state']=='settled':
                        completion=read(service/'jobs'/target/'completion.json') if target==state['session'] else None
                        if completion:confirmed=bool(completion['result'].get('termination_confirmed'))
                        else:
                            stage=next(key for key,value in state['steps'].items() if value==target)
                            receipt=read(instances[stage]/'runs/deliver/supervisor/completion.json')['result']
                            confirmed=bool(receipt.get('termination_confirmed') or receipt.get('workspace',{}).get('termination_confirmed'))
                        break
                except Exception:break
                time.sleep(.1)
            if not confirmed:owners_confirmed=False;state['state']='unknown';save(root/'state.json',state)
    finally:
        vm_confirmed=vm is None
        if vm:
            try:
                initial=direct('vm','show',vm)
                if not vm_bound and initial.get('state')=='prepared' and 'active_lease' in initial and initial['active_lease'] is None and 'active_attempt' in initial and initial['active_attempt'] is None:
                    observed=initial
                elif vm_bound:
                    observed=client.request({'op':'cocoon-release','id':state['session']},credential)
                    if not observed or not observed.get('termination_confirmed'):raise RuntimeError('Original VM release is unconfirmed')
                elif setup_bound:
                    observed=client.cli('app','task-release-setup',service,root.name)
                    if not observed.get('termination_confirmed'):raise RuntimeError('Original setup release is unconfirmed')
                elif work:
                    stop_id=str(uuid.uuid4());save(root/'controller-stop.json',{'request':stop_id,'vm':vm})
                    observed=api({'operation':'stop','vm':vm},work['token'],stop_id)
                else:observed=direct('vm','stop',vm)
                save(root/'vm-release.json',observed)
                observed=direct('vm','show',vm)
                vm_confirmed=observed.get('active_lease') is None and (observed.get('state') in ('stopped','recovery_required') or (observed.get('state')=='prepared' and observed.get('active_attempt') is None))
                if observed.get('active_lease') is not None or not vm_confirmed:
                    update(state='unknown',error='VM release not confirmed')
            except Exception:
                update(state='unknown',error='VM release not confirmed')
        if api_process is not None and vm_confirmed and owners_confirmed:
            api_process.terminate()
            try:api_process.wait(timeout=15)
            except subprocess.TimeoutExpired:vm_confirmed=False
        runtime_confirmed=api_process is None or api_process.poll() is not None
        if vm_confirmed and owners_confirmed and runtime_confirmed and state['state']=='stopping':update(state=requested_outcome,termination_confirmed=True,runtime_termination_confirmed=True)
        elif not runtime_confirmed:update(state='unknown',error='Control service release not confirmed')


if __name__=='__main__':main()

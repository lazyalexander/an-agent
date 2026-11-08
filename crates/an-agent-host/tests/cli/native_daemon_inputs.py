#!/usr/bin/env python3
"""Native operator inputs and accepted-revision handoffs through the original daemon."""
import argparse, hashlib, json, signal, subprocess, sys, time
from pathlib import Path
from host_qualification import require_native_workspace


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--host',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True)
    args=p.parse_args();require_native_workspace()
    root=args.evidence.resolve();root.mkdir(mode=0o700)
    host=args.host.resolve(strict=True);service=root/'service';daemon=None
    def call(*argv,refused=False):
        result=subprocess.run([str(host),*map(str,argv)],capture_output=True,text=True,timeout=40)
        assert (result.returncode!=0)==refused,(argv,result.stdout,result.stderr)
        return json.loads(result.stdout) if result.stdout else None
    def request(value,refused=False):
        path=root/'request.json';path.write_text(json.dumps(value))
        return call('daemon','request',service,path,refused=refused)
    def approved(req):
        return request({**req,'approve':request(req)['approval_sha256']})
    def wait(fn):
        deadline=time.monotonic()+40
        while True:
            value=fn()
            if value:return value
            assert time.monotonic()<deadline,'original completion unknown; never replay'
            time.sleep(.1)
    sha=lambda b:hashlib.sha256(b).hexdigest()
    site=root/'site';site.mkdir()
    aux=root/'aux.txt';aux.write_bytes(b'-selected')
    data=root/'data.txt';data.write_bytes(b'input')
    code=b"import os\nfrom pathlib import Path\nassert Path(os.environ['AN_SPOOL_AUX_0']).suffix == '.txt'\nPath('result.bin').write_bytes(Path(os.environ['AN_SPOOL_INPUT']).read_bytes()+Path(os.environ['AN_SPOOL_AUX_0']).read_bytes())\n"
    package=root/'package';(package/'silk').mkdir(parents=True)
    (package/'silk/main.py').write_bytes(code)
    manifest={'schema':'an-native-spool-v3','name':'selected-input','version':'0.1.0','entry':'silk/main.py','files':{'silk/main.py':sha(code)},
        'outputs':['result.bin'],'binary_outputs':['result.bin'],'events':{'consumes':['artifact.accepted'],'produces':['artifact.accepted']},
        'requirements':{'runtime':'macos-native-python-v1','os':'macos','arch':'portable-source','interaction':'headless','gpu':'none','inputs':['data'],'editable':[],'dependencies':[],'capabilities':['read-input','write-declared-outputs']}}
    (package/'native-spool.json').write_text(json.dumps(manifest))
    exe=Path(sys.executable).resolve(strict=True)
    runtime={'schema':'an-native-python-runtime-v1','executable':str(exe),'executable_sha256':sha(exe.read_bytes()),
        'stdlib':str(Path(sys.base_prefix).resolve()),'site':str(site),'site_sha256':sha(b'{}'),'dependencies':[], 'inputs':[str(aux)]}
    selection=root/'runtime.json';selection.write_text(json.dumps(runtime))
    bad=root/'bad.json';bad.write_text(json.dumps({**runtime,'graphics':'unknown'}))
    call('spool-load',package,root/'bad-instance','--runtime',bad,refused=True)
    # A release request cannot grant graphics access without the operator selection.
    metal=root/'metal';(metal/'silk').mkdir(parents=True);(metal/'silk/main.py').write_bytes(code)
    manifest['requirements']['gpu']='metal';(metal/'native-spool.json').write_text(json.dumps(manifest))
    call('spool-load',metal,root/'unapproved-metal','--runtime',selection,refused=True)
    a,b=root/'a',root/'b'
    for instance in [a,b]:call('spool-load',package,instance,'--runtime',selection)
    call('daemon','init',service,1)
    try:
        with (root/'service.log').open('w') as log:daemon=subprocess.Popen([str(host),'daemon','serve',str(service)],stdout=log,stderr=log)
        wait(lambda:(service/'control.sock').exists())
        for instance in [a,b]:request({'op':'register','instance':str(instance)})
        req={'op':'submit','instance':str(a),'run':'first','input':str(data),'seconds':20}
        request({**req,'seconds':301},refused=True)
        preview=request(req);aux.write_bytes(b'-changed')
        request({**req,'approve':preview['approval_sha256']},refused=True)
        aux.write_bytes(b'-selected')
        task=approved(req)['id']
        wait(lambda:request({'op':'task','id':task})['state']=='settled')
        assert request({'op':'task','id':task})['outcome']=='succeeded'
        approved({'op':'accept','instance':str(a),'run':'first'})
        inventory=call('spool-artifacts',a,'first','--output','result.bin')
        binding=inventory['output']['binding'];catalog=Path(inventory['catalog'])
        source=catalog/'revisions'/binding['revision']/'content.bin'
        next_req={'op':'submit','instance':str(b),'run':'second','input':str(source),'seconds':20,
                  'artifact':{'catalog':str(catalog),'revision':binding['revision']}}
        request({**next_req,'input':str(data)},refused=True)
        # New callers carry the accepted reference, never the host blob filename.
        logical={k:v for k,v in next_req.items() if k!='input'}
        scope={'task':'reference-check','subject':'consumer','expires_ms':int(time.time()*1000)+60000,'delegate':False,
               'permissions':[{'kind':'run-artifact','instance':str(b),'run':'second','catalog':str(catalog),'revision':binding['revision'],'seconds':20}]}
        issued=approved({'op':'grant','scope':scope})['credential']
        def scoped(value,refused=False):
            path=root/'scoped.json';path.write_text(json.dumps({'credential':issued,'request':value}))
            return call('daemon','task-request',service,path,refused=refused)
        scoped({**logical,'seconds':21},refused=True)
        scoped({**logical,'input':str(source)},refused=True)
        scoped({**logical,'artifact':{**logical['artifact'],'revision':'a'*64}},refused=True)
        preview=scoped(logical)
        task=scoped({**logical,'approve':preview['approval_sha256']})['id']
        wait(lambda:request({'op':'task','id':task})['state']=='settled')
        assert request({'op':'task','id':task})['outcome']=='succeeded'
        approved({'op':'accept','instance':str(b),'run':'second'})
        artifact=call('spool-artifacts',b,'second','--output','result.bin')
        output=root/'result.bin';call('artifact-checkout',artifact['catalog'],artifact['output']['binding']['revision'],output)
        assert output.read_bytes()==b'input-selected-selected'
        references=call('spool-input-references',b,'second')
        (root/'summary.json').write_text(json.dumps({'passed':True,'changed_aux_refused':True,'artifact_path_mismatch_refused':True,'logical_reference_authorized':True,'widened_reference_refused':True,'source_revision':binding['revision'],'input_references':references},indent=2))
    finally:
        if daemon is not None:
            daemon.send_signal(signal.SIGTERM);daemon.wait(timeout=10)


if __name__=='__main__': main()

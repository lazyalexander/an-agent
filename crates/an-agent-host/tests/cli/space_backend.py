#!/usr/bin/env python3
"""Public CLI SPACE routing with local transport fixtures, not a cloud claim."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host',type=Path,required=True)
    parser.add_argument('--operator-profile',type=Path,help='Explicitly use a live operator SPACE service instead of local fixtures')
    parser.add_argument('--evidence',type=Path,required=True)
    args=parser.parse_args()
    host=args.host.resolve(strict=True); root=args.evidence.resolve();root.mkdir(mode=0o700)
    source=Path(__file__).resolve().parents[2]/'src/execution/backend/space_fixture.py'
    fixture=root/'client.py';fixture.write_bytes(source.read_bytes())
    python=Path(sys.executable).resolve(strict=True)
    token=root/'token';token.write_text('ok');token.chmod(0o600)
    sha=lambda data:hashlib.sha256(data).hexdigest()
    profile=root/'profile.json'
    config={'python':str(python),'python_sha256':sha(python.read_bytes()),
        'client':str(fixture),'client_sha256':sha(fixture.read_bytes()),'token_file':str(token),
        'api_url':'http://127.0.0.1:1','data_plane_url':'http://127.0.0.1:2',
        'template':'seed@sha256:'+'ab'*32,'control_seconds':30}
    if args.operator_profile:
        config=json.loads(args.operator_profile.resolve(strict=True).read_text())
    profile.write_text(json.dumps(config))
    calls=[]
    # Allow the host to finish all bounded control phases, including owned cleanup.
    timeout=(config["control_seconds"]+10)*4+30 if args.operator_profile else 60
    def call(*command,refused=False):
        result=subprocess.run([str(host),*map(str,command)],capture_output=True,text=True,timeout=timeout)
        calls.append({'command':list(map(str,command)),'exit':result.returncode,'stdout':result.stdout,'stderr':result.stderr})
        (root/'calls.json').write_text(json.dumps(calls,indent=2))
        if refused:
            assert result.returncode!=0
            return
        assert result.returncode==0,result.stderr
        return json.loads(result.stdout.splitlines()[-1])
    deployment=root/'deployment';deployment.mkdir()
    program=b'#!/bin/sh\nprintf accepted > output.txt\n';(deployment/'program').write_bytes(program)
    (deployment/'spool.yaml').write_text('v: 1\nkind: spool\nname: space-cli\nversion: 1.0.0\nsummary: SPACE CLI fixture\nsort: beat\nconstructor: host\nhost: an-agent-file-v1\neffect:\n  file: { op: rw, path: /work, recursive: true }\n  memory: { op: ignore }\n  net: none\n  proc: spawn\n  model: { op: none }\n  flow: both\ninverse: irreversible\nrequires: []\nconsumes: [kit.request]\nproduces: [kit.execute]\nconfig:\n  profile: file-kit-v1\n  executable_sha256: "'+sha(program)+'"\n')
    store=root/'store';release=call('register',deployment,store)['registered']
    input_path=root/'input.txt';input_path.write_text('input')
    preview=call('run-space',profile,store,release,input_path)
    assert preview['execute'] is False and preview['context']['sha256']==sha(b'input')
    assert not (store/'runs').exists()
    result=call('run-space',profile,store,release,input_path,'--execute')
    receipt_path=Path(result['receipt']);receipt=json.loads(receipt_path.read_text())
    assert receipt['space']['termination_confirmed'] is True
    assert receipt['space']['artifact_accepted'] is True
    assert (receipt_path.parent/'artifact.txt').read_bytes()==b'accepted'
    assert (receipt_path.parent/'audit.json').is_file()
    config['client_sha256']='00'*32;profile.write_text(json.dumps(config))
    call('run-space',profile,store,release,input_path,'--execute',refused=True)
    assert len(list((store/'runs').iterdir()))==1
    scope='LIVE operator service' if args.operator_profile else 'local fixtures only'
    print('SPACE CLI preview, shared acceptance and changed-tool refusal passed; '+scope)

if __name__=='__main__':main()

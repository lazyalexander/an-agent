"""Exercise first-run and retry decisions without modifying the shared service."""
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import patch

source = Path(__file__).resolve().parents[2] / 'src/application/prepare.py'
spec = importlib.util.spec_from_file_location('prepare', source)
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class Setup(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        home = Path(tmp.name)
        self.env = home/'environment'
        self.env.mkdir()
        self.install = home/'Library/Application Support/an-agent/container/test'
        mapping = {'container':'bin/container', 'container_apiserver':'bin/container-apiserver'}
        for key, name in [('container_images','container-core-images'),('container_runtime_linux','container-runtime-linux'),('container_network_vmnet','container-network-vmnet')]:
            mapping[key] = f'libexec/container/plugins/{name}/bin/{name}'
        files = {}
        for name in mapping.values():
            p=self.install/name
            p.parent.mkdir(parents=True,exist_ok=True)
            p.write_bytes(name.encode())
            files[name]={'sha256':prepare.sha(p)}
        (self.install/'installation.json').write_text(json.dumps({'files':files}))
        components=dict(mapping,backend='backend',container_init='init',container_image='work')
        (self.env/'runtime.json').write_text(json.dumps({'components':components,'files':files}))
        (self.env/'backend').write_text(json.dumps({'image':'work@sha256:work'}))
        self.tags={}
        self.calls=[]
        self.live=False
        for obj,key,val in [(Path,'home',lambda:home),(prepare,'run',self.run_cli),(prepare,'checked',self.checked),(prepare,'reference',lambda p:(p.name,p.name+'@sha256:'+p.name,p.name))]:
            mock=patch.object(obj,key,val);mock.start();self.addCleanup(mock.stop)

    def run_cli(self,args):
        if args[0]=='/bin/launchctl':
            return CompletedProcess(args,0 if self.live else 1, f'program = {self.install}/bin/container-apiserver\npath = /tmp/service/apiserver/server.plist','Could not find service')
        digest=self.tags.get(args[-1])
        return CompletedProcess(args,0 if digest else 1,json.dumps([{'configuration':{'descriptor':{'digest':digest}}}]),'image not found')

    def checked(self,args):
        self.calls.append(args)
        if args[1]=='system':self.live=True
        elif args[2]=='load':
            name=args[-1].name
            self.tags[name]=name
            if name=='work' and self.fail:raise ValueError('interrupted')
        else:self.tags[args[-1]]=self.tags[args[-2]]

    def execute(self,mode):
        with patch.object(sys,'argv',['prepare',str(self.env),mode,json.dumps(['start','container_init','container_image'])]):
            return prepare.main()

    def test_retry_and_duplicate_submit(self):
        self.assertEqual(self.execute('preview'),['start','container_init','container_image'])
        self.fail=True
        with self.assertRaisesRegex(ValueError,'interrupted'):self.execute('apply')
        self.assertEqual(self.execute('preview'),['container_image'])
        self.fail=False
        self.assertEqual(self.execute('apply'),[])
        count=len(self.calls)
        self.assertEqual(self.execute('apply'),[])
        self.assertEqual(len(self.calls),count)

    def test_conflicting_tag_is_preserved(self):
        self.live=True
        self.tags['init']='another'
        with self.assertRaisesRegex(ValueError,'Conflicting image tag'):self.execute('apply')
        self.assertEqual(self.calls,[])

    def test_busy_observer_does_not_submit(self):
        lock=(self.install.parent/'.prepare.lock').open('a+')
        with lock:
            prepare.fcntl.flock(lock,prepare.fcntl.LOCK_EX)
            self.assertEqual(self.execute('apply'),['busy'])
        self.assertEqual(self.calls,[])


if __name__=='__main__':
    unittest.main()

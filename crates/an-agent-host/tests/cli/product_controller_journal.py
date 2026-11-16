#!/usr/bin/env python3
"""Fault the trusted controller's request/reply windows without a provider loop."""
import importlib.util
from pathlib import Path
import tempfile
import socket
import threading
import unittest
from unittest.mock import patch

SOURCE=Path(__file__).resolve().parents[2]/'src/application/tasks/controller.py'
SPEC=importlib.util.spec_from_file_location('product_controller',SOURCE)
controller=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(controller)


class JournalFaults(unittest.TestCase):
    def test_original_intent_precedes_submission_and_blocks_unknown_replay(self):
        for stage in ('primary','intermediate','final'):
            for fault in ('before-enqueue','after-enqueue','result-publication'):
                with self.subTest(stage=stage,fault=fault),tempfile.TemporaryDirectory() as directory:
                    root=Path(directory);client=controller.Client('unused','unused',root)
                    digest='a'*64;calls=[];enqueued=[]
                    request={'op':'harness' if stage=='primary' else 'submit','instance':'native','run':'deliver'}
                    credential={'id':'original','secret':'private-fixture'}
                    intent=root/'intents'/(stage+'.json')
                    def exchange(value,authority):
                        if 'approve' not in value:
                            return {'approval_sha256':digest,'plan':{'task_sha256':digest}}
                        self.assertTrue(intent.is_file(),'Effect was submitted before durable intent')
                        record=controller.read(intent)
                        self.assertEqual(record['request'],digest)
                        self.assertEqual(record['grant'],authority['id'])
                        calls.append(value)
                        if fault=='before-enqueue':raise ConnectionError('request not delivered')
                        enqueued.append('one-original')
                        if fault=='after-enqueue':raise ConnectionError('original reply lost')
                        return {'id':'one-original'}
                    original_save=controller.save
                    def persist(path,value):
                        if str(path).endswith('-result.json'):
                            raise OSError('receipt publication interrupted')
                        original_save(path,value)
                    with patch.object(client,'request',side_effect=exchange),patch.object(controller,'save',side_effect=persist):
                        with self.assertRaises((ConnectionError,OSError)):
                            client.approve(request,credential,record=stage)
                        with self.assertRaisesRegex(RuntimeError,'reconcile original work'):
                            client.approve(request,credential,record=stage)
                    self.assertTrue(controller.unprojected_execution(root,{'session':None,'steps':{}}))
                    self.assertFalse(controller.unprojected_execution(root,{'session':'one-original' if stage=='primary' else None,'steps':{stage:'one-original'}}))
                    self.assertEqual(len(calls),1)
                    self.assertEqual(len(enqueued),0 if fault=='before-enqueue' else 1)
                    self.assertFalse((root/'intents'/(stage+'-result.json')).exists())
                    self.assertNotIn('private-fixture',intent.read_text())

    def test_spool_execution_retains_original_session_ownership_for_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);client=controller.Client('unused','unused',root)
            with patch.object(client,'request',side_effect=[{'approval_sha256':'a'*64}, {'id':'original'}]):
                client.approve({'op':'submit','instance':'native','run':'deliver'},
                               {'id':'spool-grant'},record='intermediate',owner_grant='primary-grant')
            intent=controller.read(root/'intents/intermediate.json')
            self.assertEqual(intent['grant'],'primary-grant')
            self.assertEqual(intent['execution_grant'],'spool-grant')
            self.assertEqual(controller.read(root/'intents/intermediate-result.json')['id'],'original')

    def test_missing_request_identity_cannot_submit_effect(self):
        with tempfile.TemporaryDirectory() as directory:
            client=controller.Client('unused','unused',Path(directory))
            with patch.object(client,'request',return_value={'approval_sha256':'a'*64}) as exchange:
                with self.assertRaisesRegex(ValueError,'identity unavailable'):
                    client.approve({'op':'harness'},{'id':'original'},record='primary')
                self.assertEqual(exchange.call_count,1)


class ControlReadiness(unittest.TestCase):
    def test_retained_socket_inode_does_not_prove_control_readiness(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory).resolve()/'control.sock'
            with socket.socket(socket.AF_UNIX) as stale:stale.bind(str(path))
            self.assertTrue(path.exists())
            refused=threading.Event();failures=[];probes=[]
            def server():
                try:
                    if not refused.wait(3):raise AssertionError('stale endpoint was accepted without a probe')
                    path.unlink()
                    with socket.socket(socket.AF_UNIX) as listener:
                        listener.bind(str(path));listener.listen(1);listener.settimeout(3)
                        client,_=listener.accept()
                        with client:client.sendall(b'ready')
                except BaseException as error:failures.append(error)
            worker=threading.Thread(target=server);worker.start()
            def probe():
                probes.append(True)
                try:
                    with socket.socket(socket.AF_UNIX) as client:
                        client.settimeout(1);client.connect(str(path))
                        self.assertEqual(client.recv(5),b'ready')
                except OSError:
                    refused.set();raise
            process=type('Process',(),{'poll':lambda self:None})()
            try:controller.wait_control(path,process,probe,lambda:None)
            finally:refused.set();worker.join(timeout=4)
            self.assertFalse(worker.is_alive());self.assertFalse(failures,failures)
            self.assertGreaterEqual(len(probes),2)

    def test_exited_owner_refuses_a_still_present_endpoint(self):
        process=type('Process',(),{'poll':lambda self:0})()
        probe=unittest.mock.Mock()
        with self.assertRaisesRegex(RuntimeError,'service stopped'):
            controller.wait_control(Path('/unused'),process,probe,lambda:None)
        probe.assert_not_called()


class ContextScopes(unittest.TestCase):
    def test_spool_writes_are_independent_and_credentials_do_not_enter_context(self):
        import runpy
        bind = runpy.run_path(str(SOURCE.with_name('context_support.py')))['bind']
        issued = []
        class Client:
            def approve(self, request):
                issued.append(request['scope'])
                return {'credential': {'id': 'grant-' + str(len(issued)), 'secret': 'private-fixture'}}
            def cli(self, *args):
                return {'release': 'a' * 64}
            def request(self, request):
                self_test.assertEqual(len(request['task']['scope']['turn_context']['spool_bindings']), 2)
                return {'kind': 'harness', 'profile_sha256': 'b' * 64}
        self_test = self
        task = {'scope': {}, 'prompt': 'Preserve facts.'}
        scope = {'task': 'one', 'subject': 'primary', 'expires_ms': 100, 'delegate': True,
                 'permissions': [{'kind': 'accept', 'instance': '/intermediate', 'run': 'deliver'},
                                 {'kind': 'accept', 'instance': '/final', 'run': 'deliver'},
                                 {'kind': 'read-input', 'path': '/source', 'sha256': 'c' * 64}]}
        credentials = bind(Client(), task, scope,
                           {'intermediate': Path('/intermediate'), 'final': Path('/final')},
                           {name: Path('/' + name) for name in ('extract', 'intermediate', 'final')},
                           lambda path: 'd' * 64)
        self.assertEqual(set(credentials), {'intermediate', 'final'})
        self.assertNotIn('private-fixture', str(task))
        self.assertFalse(any(p['kind'] == 'accept' for p in scope['permissions']))
        for index, instance in enumerate(('/intermediate', '/final')):
            self.assertFalse(issued[index]['delegate'])
            self.assertEqual([p['instance'] for p in issued[index]['permissions'] if p['kind'] == 'accept'], [instance])


class CocoonFailures(unittest.TestCase):
    def test_backpressure_is_retained_without_replay_or_disabling_protection(self):
        import runpy
        call=runpy.run_path(str(SOURCE.with_name('cocoon_control.py')))['call']
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            direct=unittest.mock.Mock(return_value={'request':'one','caller':'product-task',
                'result':{'Err':'host protection backpressure rejects new execution'}})
            with self.assertRaisesRegex(RuntimeError,'memory protection blocked'):
                call(direct,controller.save,root,{'operation':'start','vm':'fixture'},request_id='one')
            direct.assert_called_once()
            self.assertEqual(controller.read(root/'cocoon-errors/one.json')['operation'],'start')
            self.assertNotIn('grant',controller.read(root/'cocoon-errors/one.json'))

    def test_foreign_response_is_not_accepted(self):
        import runpy
        call=runpy.run_path(str(SOURCE.with_name('cocoon_control.py')))['call']
        with tempfile.TemporaryDirectory() as directory:
            direct=unittest.mock.Mock(return_value={'request':'foreign','caller':'product-task','result':{'Ok':{}}})
            with self.assertRaisesRegex(RuntimeError,'identity mismatch'):
                call(direct,controller.save,Path(directory),{'operation':'start'},request_id='one')


if __name__=='__main__':unittest.main()

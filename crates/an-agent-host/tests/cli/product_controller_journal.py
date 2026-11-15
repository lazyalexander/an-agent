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


if __name__=='__main__':unittest.main()

#!/usr/bin/env python3
"""Preserve backend ownership when a preparation reply is lost."""
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SOURCE = Path(__file__).resolve().parents[2] / 'src/application/tasks/cocoon_backend.py'
Backend = runpy.run_path(str(SOURCE))['CocoonBackend']


class PreparationOwnership(unittest.TestCase):
    def test_lost_create_reply_retains_identity_and_only_releases_owned_process(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state = {'state': 'stopping', 'session': None}
            client = Mock()
            client.cli.return_value = {'settled': True}
            process = Mock(pid=123)
            process.poll.return_value = 0

            def save(path, value):
                path.write_text(json.dumps(value))

            backend = Backend(root, {key: key for key in ('cocoon', 'runner', 'kernel', 'initrd', 'root_image')},
                              {}, root, root, client, save, None, lambda: None,
                              lambda **values: state.update(values), lambda *args: None, {})
            effects = []

            def direct(*args):
                if args[:2] == ('image', 'import'):
                    return {'image': 'boot'}
                if args[:2] == ('vm', 'create'):
                    effects.append(args[2])
                    raise RuntimeError('lost create reply')
                if args[:2] == ('vm', 'show'):
                    self.assertEqual(args[2], effects[0])
                    return {'state': 'prepared', 'active_lease': None, 'active_attempt': None}
                self.fail('Unexpected mutation: ' + repr(args))

            backend.direct = direct
            with patch.dict(Backend.prepare.__globals__['subprocess'].__dict__, Popen=Mock(return_value=process)):
                with self.assertRaisesRegex(RuntimeError, 'lost create reply'):
                    backend.prepare()
            self.assertEqual(backend.vm, effects[0])
            self.assertEqual(json.loads((root / 'setup.json').read_text())['vm'], effects[0])
            backend.cleanup(state, None, False, False, True, 'failed')
            self.assertEqual(effects, [backend.vm])
            process.terminate.assert_called_once()
            self.assertEqual(state['state'], 'failed')
            self.assertTrue(state['termination_confirmed'])


if __name__ == '__main__':
    unittest.main()

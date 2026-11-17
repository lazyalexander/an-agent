"""Reject changed provenance before invoking a compiler or emitting evidence."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import container_build as build


class BuildPins(unittest.TestCase):
    def test_changed_inputs_refuse_before_build(self):
        lock = json.loads(Path(build.__file__).with_name('container-build-lock.json').read_text())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock['package_resolved_sha256'] = hashlib.sha256(b'lock').hexdigest()
            (root / 'Package.resolved').write_bytes(b'lock')
            lock_path = root / 'lock.json'
            lock_path.write_text(json.dumps(lock))
            commands = {
                ('git', 'rev-parse', 'HEAD'): lock['commit'],
                ('git', 'remote', 'get-url', 'origin'): lock['repository'],
                ('git', 'status', '--porcelain', '--untracked-files=normal'): '',
                ('/usr/bin/swift', '--version'): lock['swift'],
                ('/usr/bin/xcodebuild', '-version'): lock['xcode'],
            }
            with patch.object(build, 'output', side_effect=lambda source, *args: commands[args]):
                build.validate(root, lock)
                for command in commands:
                    with self.subTest(command=command):
                        prior = commands[command]
                        commands[command] = 'changed'
                        with patch.object(build.subprocess, 'run') as compiler:
                            with self.assertRaises(ValueError):
                                build.build(root, root / 'evidence', lock_path)
                            compiler.assert_not_called()
                        self.assertFalse((root / 'evidence').exists())
                        commands[command] = prior
                (root / 'Package.resolved').write_bytes(b'changed')
                with self.assertRaises(ValueError):
                    build.validate(root, lock)


if __name__ == '__main__':
    unittest.main()

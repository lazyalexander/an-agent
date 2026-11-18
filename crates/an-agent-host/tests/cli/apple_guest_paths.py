#!/usr/bin/env python3
"""Exercise guest transfer containment with real descriptor-relative filesystem calls."""
import contextlib
import io
import os
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SOURCE = Path(__file__).resolve().parents[2] / 'src/execution/guest/apple/guest.py'
with patch('sys.stdin', io.StringIO('{"identity":{},"operation":{"kind":"invalid"}}')):
    with contextlib.redirect_stdout(io.StringIO()):
        guest = runpy.run_path(str(SOURCE))


class TransferPaths(unittest.TestCase):
    def test_parent_and_leaf_links_cannot_write_outside_workspace(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / 'workspace'
            workspace.mkdir()
            outside = root / 'outside'
            outside.mkdir()
            victim = outside / 'victim'
            victim.write_bytes(b'original')
            (workspace / 'parent').symlink_to(outside, target_is_directory=True)
            (workspace / 'leaf').symlink_to(victim)
            original_open = os.open

            def open_root(path, *args, **kwargs):
                return original_open(workspace if path == '/workspace' else path, *args, **kwargs)

            with patch.object(os, 'open', side_effect=open_root):
                for target in ('parent/victim', 'leaf', '../outside/victim', './leaf', 'parent//victim'):
                    with self.subTest(target=target), self.assertRaises((OSError, ValueError)):
                        guest['execute']({'kind': 'write_bytes', 'path': '/workspace/' + target,
                                          'offset': 0, 'content_hex': b'changed'.hex(), 'sync': True})
                self.assertEqual(victim.read_bytes(), b'original')
                guest['execute']({'kind': 'write_bytes', 'path': '/workspace/regular',
                                  'offset': 0, 'content_hex': '00ff80', 'sync': True})
                result = guest['execute']({'kind': 'read_chunk', 'path': '/workspace/regular',
                                           'offset': 0, 'max_bytes': 8})
                self.assertEqual(result['content_hex'], '00ff80')
                self.assertTrue(result['eof'])


if __name__ == '__main__':
    unittest.main()

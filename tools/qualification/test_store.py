"""State publication, ownership and exclusive-owner regression tests."""
import tempfile
import unittest
from pathlib import Path
from store import lock, private_root, read, write
from candidate import inventory


class StateTests(unittest.TestCase):
    def test_atomic_replacement_and_private_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = private_root(directory)
            write(root / 'state.json', {'state': 'queued'})
            write(root / 'state.json', {'state': 'unknown'})
            self.assertEqual(read(root / 'state.json'), {'state': 'unknown'})
            self.assertEqual((root / 'state.json').stat().st_mode & 0o777, 0o600)
            self.assertEqual(len(list(root.iterdir())), 1)

    def test_duplicate_owner_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = private_root(directory)
            with lock(root):
                with self.assertRaises(BlockingIOError):
                    with lock(root):
                        pass

    def test_symlink_input_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'escape').symlink_to('/etc/passwd')
            with self.assertRaises(ValueError):
                inventory(root)

    def test_public_root_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); root.chmod(0o755)
            with self.assertRaises(ValueError):
                private_root(root)


if __name__ == '__main__':
    unittest.main()

"""Freeze explicit clean Git sources and selected immutable inputs."""
import os
from pathlib import Path
import shutil
import subprocess
import uuid
from store import digest, read, write


def git(source, *args):
    return subprocess.check_output(['git', '-C', str(source), *args], text=True).strip()


def inventory(root):
    result = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            raise ValueError('Candidate symlinks are not supported: ' + str(path))
        if path.is_file() and '.git' not in path.relative_to(root).parts:
            result[path.relative_to(root).as_posix()] = digest(path)
    return result


def freeze(root, config_path):
    config = read(config_path)
    required = {'sources', 'files', 'external', 'model'}
    if set(config) != required or set(config['sources']) != {'host', 'spool', 'shuttle'}:
        raise ValueError('Expected sources(host, spool, shuttle), files, external and model')
    if set(config['files']) != {'host_binary', 'codex', 'mechanical_fixtures', 'mechanical_site', 'python_site'}:
        raise ValueError('All frozen runtime inputs are required')
    identity = uuid.uuid4().hex
    destination = root / 'candidates' / identity
    destination.mkdir(parents=True, mode=0o700)
    sources = {}
    try:
        for name, source in config['sources'].items():
            if git(source, 'status', '--porcelain'):
                raise ValueError('Source must be clean: ' + name)
            revision = git(source, 'rev-parse', 'HEAD')
            target = destination / name
            subprocess.run(['git', 'clone', '--no-local', '--no-checkout', str(Path(source).resolve()), str(target)], check=True, capture_output=True)
            subprocess.run(['git', '-C', str(target), '-c', 'core.hooksPath=/dev/null', 'checkout', '--detach', revision], check=True, capture_output=True)
            sources[name] = revision
        for name, source in config['files'].items():
            if name not in {'host_binary', 'codex', 'mechanical_fixtures', 'mechanical_site', 'python_site'}:
                raise ValueError('Unknown frozen input: ' + name)
            source = Path(source).resolve(strict=True)
            if source.is_dir():
                inventory(source)
                shutil.copytree(source, destination / name)
            else:
                shutil.copy2(source, destination / name)
        external = {str(Path(p).resolve(strict=True)): digest(p) for p in config['external']}
        manifest = {'schema': 1, 'id': identity, 'sources': sources, 'model': config['model'],
                    'external': external, 'files': inventory(destination)}
        write(destination / 'manifest.json', manifest)
        return {'candidate': identity, 'manifest_sha256': digest(destination / 'manifest.json')}
    except Exception:
        # Retain incomplete evidence; it has no manifest and cannot be admitted.
        raise


def verify(root, identity, expected):
    if len(identity) != 32 or any(c not in '0123456789abcdef' for c in identity):
        raise ValueError('Invalid candidate identity')
    path = root / 'candidates' / identity
    if digest(path / 'manifest.json') != expected:
        raise ValueError('Candidate manifest changed')
    manifest = read(path / 'manifest.json')
    actual = inventory(path)
    actual.pop('manifest.json', None)
    if actual != manifest['files']:
        raise ValueError('Frozen candidate changed')
    for name, expected_hash in manifest['external'].items():
        if digest(name) != expected_hash:
            raise ValueError('External dependency changed')
    return path, manifest

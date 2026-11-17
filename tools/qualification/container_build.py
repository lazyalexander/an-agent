"""Build pinned fork bytes without starting, stopping, or adopting services."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


PRODUCTS = (
    'container', 'container-apiserver', 'container-runtime-linux',
    'container-network-vmnet', 'container-core-images', 'container-core-clusters',
    'machine-apiserver', 'cluster', 'cluster-hoststore', 'container-build',
    'container-builder',
)


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(65536), b''):
            value.update(block)
    return value.hexdigest()


def output(source, *args):
    return subprocess.check_output(args, cwd=source, text=True,
                                   stderr=subprocess.STDOUT).strip()


def validate(source, lock):
    if lock['schema'] != 'an-container-build-lock-v1':
        raise ValueError('Unsupported build lock')
    expected = {
        'commit': output(source, 'git', 'rev-parse', 'HEAD'),
        'repository': output(source, 'git', 'remote', 'get-url', 'origin'),
        'package_resolved_sha256': digest(source / 'Package.resolved'),
        'swift': output(source, '/usr/bin/swift', '--version'),
        'xcode': output(source, '/usr/bin/xcodebuild', '-version'),
    }
    for key, actual in expected.items():
        if actual != lock[key]:
            raise ValueError('Pinned build input changed: ' + key)
    if output(source, 'git', 'status', '--porcelain', '--untracked-files=normal'):
        raise ValueError('Source checkout must be clean')
    if lock['configuration'] != 'release':
        raise ValueError('Unsupported build configuration')


def build(source, destination, lock_path):
    lock = json.loads(lock_path.read_bytes())
    validate(source, lock)
    # Reserve evidence before building. A failed build leaves no success manifest.
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    command = ['/usr/bin/swift', 'build', '--disable-automatic-resolution',
               '-c', lock['configuration']]
    environment = {key: os.environ[key] for key in ('HOME', 'TMPDIR') if key in os.environ}
    environment.update(PATH='/usr/bin:/bin:/usr/sbin:/sbin',
                       GIT_COMMIT=lock['commit'], RELEASE_VERSION='0.0.0-an-agent')
    with (destination / 'build.log').open('xb') as log:
        subprocess.run(command, cwd=source, env=environment,
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    validate(source, lock)
    directory = Path(output(source, '/usr/bin/swift', 'build', '-c',
                            lock['configuration'], '--show-bin-path'))
    binaries = {}
    for name in PRODUCTS:
        path = directory / name
        if not path.is_file() or path.is_symlink():
            raise ValueError('Missing regular build product: ' + name)
        binaries[name] = {'sha256': digest(path), 'bytes': path.stat().st_size}
    value = {'schema': 'an-container-build-v1', 'source': lock,
             'build_lock_sha256': digest(lock_path), 'command': command,
             'binaries': binaries, 'runtime_qualified': False,
             'cluster_shim_included': False}
    with (destination / 'build.json').open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')
    return value


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('evidence', type=Path)
    parser.add_argument('--lock', type=Path,
                        default=Path(__file__).with_name('container-build-lock.json'))
    args = parser.parse_args()
    print(json.dumps(build(args.source.resolve(), args.evidence, args.lock)))

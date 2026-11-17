"""Kit: bounded guest proposals. Host observations and acceptance remain authoritative."""
import json
import os
import resource
import signal
import stat
import subprocess
import sys
import tempfile


def path(value):
    if (not isinstance(value, str) or not value.startswith('/workspace/')
            or any(part in ('', '.', '..') for part in value.split('/')[2:])):
        raise ValueError('path outside workspace')
    return value


def open_workspace(target, flags):
    """Walk pinned directory descriptors; guest-created parent links cannot escape."""
    parts = path(target).split('/')[2:]
    directory = os.open('/workspace', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=directory)
            os.close(directory)
            directory = child
        return os.open(parts[-1], flags, 0o600, dir_fd=directory)
    finally:
        os.close(directory)


def execute(op):
    kind = op['kind']
    if kind == 'exec':
        argv = op['argv']
        if not argv or len(argv) > 128 or any(not isinstance(a, str) or len(a) > 8192 for a in argv):
            raise ValueError('invalid argv')
        cwd = op['cwd']
        if cwd != '/workspace':
            path(cwd)
        with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
            def limits():
                resource.setrlimit(resource.RLIMIT_FSIZE, (4194304, 4194304))
            process = subprocess.Popen(argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=out,
                stderr=err, start_new_session=True, preexec_fn=limits,
                env={'PATH': '/usr/local/bin:/usr/bin:/bin', 'LANG': 'C'})
            timed_out = False
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                timed_out = True
            finally:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
            out.seek(0); err.seek(0)
            stdout, stderr = out.read(8193), err.read(8193)
            if len(stdout) > 8192 or len(stderr) > 8192:
                raise ValueError('output limit')
            return dict(kind='exited', code=process.returncode, timed_out=timed_out,
                        stdout=stdout.decode(), stderr=stderr.decode())
    target = path(op['path'])
    offset = op['offset']
    if not isinstance(offset, int) or not 0 <= offset <= 4194304:
        raise ValueError('offset limit')
    if kind == 'write_bytes':
        data = bytes.fromhex(op['content_hex'])
        if len(data) > 8192 or offset + len(data) > 4194304:
            raise ValueError('write limit')
        flags = os.O_WRONLY | os.O_NOFOLLOW | os.O_NONBLOCK
        if offset == 0:
            flags |= os.O_CREAT
    elif kind == 'read_chunk':
        count = op['max_bytes']
        if not isinstance(count, int) or not 1 <= count <= 8192:
            raise ValueError('read limit')
        flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
    else:
        raise ValueError('unsupported operation')
    fd = open_workspace(target, flags)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise ValueError('regular file required')
        if kind == 'write_bytes' and offset == 0:
            os.ftruncate(fd, 0)
        os.lseek(fd, offset, os.SEEK_SET)
        if kind == 'write_bytes':
            view = memoryview(data)
            while view:
                written = os.write(fd, view)
                if written <= 0:
                    raise OSError('incomplete write')
                view = view[written:]
            if op['sync']:
                os.fsync(fd)
            return dict(kind='written')
        data = os.read(fd, count + 1)
        return dict(kind='chunk', content_hex=data[:count].hex(),
                    next_offset=offset + min(count, len(data)), eof=len(data) <= count)
    finally:
        os.close(fd)


request = json.load(sys.stdin)
try:
    result = {'Ok': execute(request['operation'])}
except Exception:
    result = {'Err': 'guest operation rejected'}
print(json.dumps({'identity': request['identity'], 'result': result}))

"""macOS process birth identity; PID reuse never identifies an original owner."""
import ctypes
import os
import sys


class BSD(ctypes.Structure):
    _fields_ = [('flags', ctypes.c_uint32), ('status', ctypes.c_uint32),
                ('xstatus', ctypes.c_uint32), ('pid', ctypes.c_uint32),
                ('ppid', ctypes.c_uint32), ('ids', ctypes.c_uint32 * 7),
                ('comm', ctypes.c_char * 16), ('name', ctypes.c_char * 32),
                ('tail', ctypes.c_uint32 * 6), ('seconds', ctypes.c_uint64),
                ('microseconds', ctypes.c_uint64)]


def identity(pid):
    if sys.platform != 'darwin':
        raise RuntimeError('Native qualification ownership requires macOS')
    info = BSD()
    library = ctypes.CDLL('/usr/lib/libproc.dylib')
    count = library.proc_pidinfo(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info))
    if count != ctypes.sizeof(info):
        raise RuntimeError('Original process identity unavailable')
    return {'pid': info.pid, 'started_seconds': info.seconds,
            'started_microseconds': info.microseconds}


def exited(original):
    try:
        os.kill(original['pid'], 0)
    except ProcessLookupError:
        return True
    return identity(original['pid']) != original

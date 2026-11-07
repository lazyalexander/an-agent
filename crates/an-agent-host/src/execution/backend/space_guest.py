"""Root-owned collector for the fixed file-kit ABI; kit executes unprivileged."""
import base64, json, os, signal, stat, subprocess, sys, tempfile
from pathlib import Path
p = json.loads(base64.b64decode(sys.argv[1], validate=True))
root = Path(tempfile.mkdtemp(prefix='space-file-kit-')); root.chmod(0o755)
program = root/'program'; program.write_bytes(base64.b64decode(p['program'], validate=True)); program.chmod(0o555)
work = root/'work'; work.mkdir(); work.chmod(0o555)
output=work/'output.txt'; output.touch(); os.chown(output,65534,65534); output.chmod(0o600)
input_file=work/'input.txt'; input_file.write_bytes(base64.b64decode(p['input'],validate=True)); input_file.chmod(0o444)
proc = subprocess.Popen([str(program)], cwd=work, env={'PATH':'/usr/bin:/bin','LANG':'C'},
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    user=65534,group=65534,extra_groups=[],start_new_session=True)
try:
    code=proc.wait(timeout=5)
finally:
    try: os.killpg(proc.pid,signal.SIGKILL)
    except ProcessLookupError: pass
    proc.wait()
if code != 0: raise RuntimeError('kit failed')
fd=os.open(work/'output.txt',os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
try:
    before=os.fstat(fd)
    assert stat.S_ISREG(before.st_mode) and before.st_nlink==1 and before.st_size<=65536
    data=b''
    while len(data)<=65536:
        block=os.read(fd,65537-len(data))
        if not block:break
        data+=block
    after=os.fstat(fd)
    assert len(data)<=65536 and after.st_nlink==1 and before.st_size==after.st_size==len(data)
finally: os.close(fd)
print(json.dumps({'exit_code':0,'candidate':base64.b64encode(data).decode()}))

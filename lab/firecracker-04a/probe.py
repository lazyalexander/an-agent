import argparse
import contextlib
import http.client
import json
import pathlib
import socket
import sqlite3
import statistics
import subprocess
import sys
import tempfile
import time


RECEIPT = ("revision-7", "grant-12", "call-42")


def ms(start):
    return (time.monotonic_ns() - start) / 1_000_000


class Attempt:
    def __init__(self, path):
        self.path = path
        with self.open() as db:
            db.execute("CREATE TABLE attempt (revision, grant_id, call_id, proof)")
            db.execute("INSERT INTO attempt VALUES (?, ?, ?, NULL)", RECEIPT)

    @contextlib.contextmanager
    def open(self):
        with contextlib.closing(sqlite3.connect(self.path)) as db, db:
            yield db

    def publish(self, receipt, produce):
        with self.open() as db:
            db.execute("BEGIN IMMEDIATE")
            pending = db.execute(
                "SELECT 1 FROM attempt WHERE revision = ? AND grant_id = ? "
                "AND call_id = ? AND proof IS NULL", receipt
            ).fetchone()
            if pending is None:
                return False
            db.execute("UPDATE attempt SET proof = ?", (produce(),))
        return True


class UnixConnection(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("localhost", timeout=5)
        self.path = str(path)

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.path)


class VM:
    def __init__(self, args, directory):
        self.args = args
        self.api = directory / "api"
        self.vsock = directory / "vsock"
        self.console = directory / "console"
        self.proc = None

    def wait_for(self, check):
        deadline = time.monotonic() + 8
        while self.proc.poll() is None and time.monotonic() < deadline:
            value = check()
            if value:
                return value
            time.sleep(0.002)
        raise RuntimeError(f"VM readiness failed\n{self.console.read_text(errors='replace')}")

    def start(self):
        with self.console.open("wb") as console:
            self.proc = subprocess.Popen(
                [self.args.firecracker, "--api-sock", str(self.api)],
                stdin=subprocess.DEVNULL, stdout=console, stderr=subprocess.STDOUT,
            )
        self.wait_for(self.api.exists)

    def request(self, method, path, body):
        with contextlib.closing(UnixConnection(self.api)) as conn:
            conn.request(method, path, json.dumps(body), {"Content-Type": "application/json"})
            response = conn.getresponse()
            payload = response.read()
            if response.status != 204:
                raise RuntimeError(f"{method} {path}: {response.status} {payload!r}")

    def boot(self):
        self.start()
        self.request("PUT", "/machine-config", {"vcpu_count": 1, "mem_size_mib": 128})
        self.request("PUT", "/boot-source", {
            "kernel_image_path": self.args.kernel,
            "initrd_path": self.args.initrd,
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off rdinit=/init",
        })
        self.request("PUT", "/vsock", {"guest_cid": 3, "uds_path": str(self.vsock)})
        self.request("PUT", "/actions", {"action_type": "InstanceStart"})

        def ready():
            for line in self.console.read_text(errors="replace").splitlines():
                if line.startswith("READY ") and len(line.split()) == 2:
                    return line.split()[1].encode()
            return None

        return self.wait_for(ready)

    def proof(self, receipt, nonce):
        payload = "|".join(receipt).encode() + b"\n"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(5)
            sock.connect(str(self.vsock))
            sock.sendall(b"CONNECT 4040\n")
            with sock.makefile("rb") as stream:
                if not stream.readline(128).startswith(b"OK "):
                    raise RuntimeError("vsock handshake failed")
                sock.sendall(payload)
                result = stream.readline(256)
        if result != b"PROOF " + nonce + b" " + payload:
            raise RuntimeError(f"unexpected proof {result!r}")
        return result

    def rss_kib(self):
        for line in pathlib.Path(f"/proc/{self.proc.pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
        raise RuntimeError("VMM RSS not reported")

    def stop(self):
        if self.proc is not None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=5)
            self.proc = None
        self.api.unlink(missing_ok=True)
        self.vsock.unlink(missing_ok=True)


def rejected():
    raise RuntimeError("rejected receipt reached the worker")


def trial(mode, args, directory):
    attempt = Attempt(directory / "attempt.db")
    for bad_receipt in (
        ("revision-6", *RECEIPT[1:]),
        (RECEIPT[0], "wrong-grant", RECEIPT[2]),
        (*RECEIPT[:2], "wrong-call"),
    ):
        if attempt.publish(bad_receipt, rejected):
            raise RuntimeError("accepted mismatched receipt")

    metrics = {}
    vm = VM(args, directory)
    try:
        if mode == "resident" or mode == "snapshot":
            nonce = vm.boot()
            start = time.monotonic_ns()
            vm.request("PATCH", "/vm", {"state": "Paused"})
            metrics["pause_ms"] = ms(start)
            metrics["paused_vmm_rss_kib"] = vm.rss_kib()

        if mode == "snapshot":
            state = directory / "state"
            memory = directory / "memory"
            start = time.monotonic_ns()
            vm.request("PUT", "/snapshot/create", {
                "snapshot_type": "Full",
                "snapshot_path": str(state),
                "mem_file_path": str(memory),
            })
            vm.stop()
            metrics["snapshot_and_stop_ms"] = ms(start)
            metrics["snapshot_logical_bytes"] = state.stat().st_size + memory.stat().st_size
            metrics["snapshot_allocated_bytes"] = (state.stat().st_blocks + memory.stat().st_blocks) * 512

        def produce():
            if mode == "native":
                return "|".join(RECEIPT)
            if mode == "cold":
                proof_nonce = vm.boot()
                metrics["receipt_to_guest_ready_ms"] = ms(start)
            else:
                proof_nonce = nonce
                if mode == "resident":
                    vm.request("PATCH", "/vm", {"state": "Resumed"})
                else:
                    vm.start()
                    vm.request("PUT", "/snapshot/load", {
                        "snapshot_path": str(state),
                        "mem_backend": {"backend_type": "File", "backend_path": str(memory)},
                        "resume_vm": True,
                    })
            return vm.proof(RECEIPT, proof_nonce)

        start = time.monotonic_ns()
        if not attempt.publish(RECEIPT, produce):
            raise RuntimeError("valid receipt was rejected")
        metrics["receipt_to_commit_ms"] = ms(start)
        if attempt.publish(RECEIPT, rejected):
            raise RuntimeError("duplicate receipt was published")
        return metrics
    finally:
        if vm.console.exists():
            if vm.proc is not None and vm.proc.poll() is not None:
                print(vm.console.read_text(errors="replace"), flush=True)
        vm.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--firecracker")
    parser.add_argument("--kernel")
    parser.add_argument("--initrd")
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    modes = ["native"]
    if any((args.firecracker, args.kernel, args.initrd)):
        if not all((args.firecracker, args.kernel, args.initrd)):
            parser.error("--firecracker, --kernel and --initrd must be provided together")
        modes.extend(("cold", "resident", "snapshot"))

    samples = {mode: [] for mode in modes}
    for sample in range(1, args.samples + 1):
        for mode in modes:
            with tempfile.TemporaryDirectory(prefix="fc04a-", dir="/tmp") as directory:
                try:
                    samples[mode].append(trial(mode, args, pathlib.Path(directory)))
                except Exception:
                    print(f"{mode} sample {sample} failed", file=sys.stderr, flush=True)
                    console = pathlib.Path(directory) / "console"
                    if console.exists():
                        print(console.read_text(errors="replace"), flush=True)
                    raise
    summary = {
        mode: {key: statistics.median(s[key] for s in runs) for key in runs[0]}
        for mode, runs in samples.items()
    }
    print(json.dumps({"median": summary, "samples": samples}, indent=2))


if __name__ == "__main__":
    main()

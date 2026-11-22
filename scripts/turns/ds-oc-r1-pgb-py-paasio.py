THOUGHT = r'''Exercise the wrappers end to end with a real temp file plus a stub socket to check
byte and op counts, iteration, context manager behaviour and delegation.'''

CONTENT = r'''Verifying counts, iteration and context-manager delegation.'''

CALLS = [
    {"name": "bash", "arguments": {"command": r'''python3 - <<'PY'
import io, os, tempfile
from paasio import MeteredFile, MeteredSocket

path = tempfile.mktemp()
with open(path, "wb") as raw:
    raw.write(b"one\ntwo\nthree")

with MeteredFile(open(path, "r+b")) as mf:
    print(isinstance(mf, io.BufferedRandom))
    print(mf.read(3), mf.read_bytes, mf.read_ops)
    mf.seek(0)
    print(mf.read(), mf.read_bytes, mf.read_ops)
    print(mf.read(1), mf.read_bytes, mf.read_ops)
    mf.seek(0, 2)
    print(mf.write(b"!"), mf.write_bytes, mf.write_ops)
    mf.seek(0)
    lines = list(mf)
    print(len(lines), lines[0])

class Sock:
    def __init__(self):
        self.buf = [b"abc", b"de", b""]
        self.sent = 0
        self.closed = False
    def recv(self, n, flags=0):
        return self.buf.pop(0) if self.buf else b""
    def send(self, data, flags=0):
        self.sent += len(data)
        return len(data)
    def __enter__(self):
        return "entered"
    def __exit__(self, *a):
        self.closed = True
        return False

raw_sock = Sock()
with MeteredSocket(raw_sock) as ms:
    print(ms is raw_sock, ms.recv(10), ms.recv(10), ms.recv(10))
    print(ms.recv_bytes, ms.recv_ops)
    print(ms.send(b"hello"), ms.send(b"xy"))
    print(ms.send_bytes, ms.send_ops)
print("closed:", raw_sock.closed)
os.unlink(path)
PY'''}},
]

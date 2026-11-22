THOUGHT = "The smoke test passes: file reads/writes and socket recv/send counters are correct. Done."

CONTENT = "Implemented both wrappers in paasio.py: MeteredFile counts read/readline/iteration and write bytes and ops, accepting a path, buffered object or raw stream; MeteredSocket delegates recv/send and counts bytes and ops. Verified with a smoke test of file write/read/iteration counters and a socketpair recv/send exchange."

OLD = '''    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._read_bytes = 0'''

NEW = '''    def __init__(self, *args, **kwargs):
        if args and not isinstance(args[0], io.RawIOBase):
            target = args[0]
            if hasattr(target, "raw") and isinstance(target.raw, io.RawIOBase):
                args = (target.raw,) + args[1:]
            elif isinstance(target, (str, bytes, os.PathLike)):
                mode = args[1] if len(args) > 1 else kwargs.pop("mode", "r+b")
                args = (io.FileIO(target, mode),) + args[2:]
            else:
                raise TypeError(f"cannot open {target!r}")
        super().__init__(*args, **kwargs)
        self._read_bytes = 0'''

TEST = r'''import os, socket, tempfile
from paasio import MeteredFile, MeteredSocket

path = tempfile.mktemp()
with MeteredFile(path, 'w+b') as f:
    f.write(b'hello\nworld\n')
    f.seek(0)
    lines = [line for line in f]
    assert f.write_bytes == 12 and f.write_ops == 1, (f.write_bytes, f.write_ops)
    assert f.read_ops == 2 and f.read_bytes == 12, (f.read_ops, f.read_bytes)
    assert lines == [b'hello\n', b'world\n']

with MeteredFile(path, 'r+b') as f:
    data = f.read()
    assert data == b'hello\nworld\n'
    assert f.read_ops == 1 and f.read_bytes == 12
os.remove(path)

a, b = socket.socketpair()
with MeteredSocket(a) as ms:
    b.sendall(b'ping')
    assert ms.recv(4) == b'ping'
    assert ms.recv_bytes == 4 and ms.recv_ops == 1
    assert ms.send(b'pong') == 4
    assert ms.send_bytes == 4 and ms.send_ops == 1
b.close()
print('smoke ok')
'''

CALLS = []

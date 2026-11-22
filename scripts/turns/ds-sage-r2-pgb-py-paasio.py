THOUGHT = "Implement both classes: MeteredFile by subclassing io.BufferedRandom and MeteredSocket by delegation, then verify with in-memory file and socketpair checks."
CONTENT = "Implemented both wrappers and verified them."
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "paasio.py",
            "overwrite": True,
            "content": r'''import io


class MeteredFile(io.BufferedRandom):
    """Implement using a subclassing model."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._read_bytes = 0
        self._read_ops = 0
        self._write_bytes = 0
        self._write_ops = 0

    def __enter__(self):
        return super().__enter__()

    def __exit__(self, exc_type, exc_val, exc_tb):
        return super().__exit__(exc_type, exc_val, exc_tb)

    def __iter__(self):
        return self

    def __next__(self):
        line = super().readline()
        self._read_bytes += len(line)
        self._read_ops += 1
        if line:
            return line
        raise StopIteration

    def read(self, size=-1):
        data = super().read(size)
        self._read_bytes += len(data)
        self._read_ops += 1
        return data

    @property
    def read_bytes(self):
        return self._read_bytes

    @property
    def read_ops(self):
        return self._read_ops

    def write(self, b):
        written = super().write(b)
        self._write_bytes += written
        self._write_ops += 1
        return written

    @property
    def write_bytes(self):
        return self._write_bytes

    @property
    def write_ops(self):
        return self._write_ops


class MeteredSocket:
    """Implement using a delegation model."""

    def __init__(self, socket):
        self._socket = socket
        self._recv_bytes = 0
        self._recv_ops = 0
        self._send_bytes = 0
        self._send_ops = 0

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        return self._socket.__exit__(exc_type, exc_val, exc_tb)

    def recv(self, bufsize, flags=0):
        data = self._socket.recv(bufsize, flags)
        self._recv_bytes += len(data)
        self._recv_ops += 1
        return data

    @property
    def recv_bytes(self):
        return self._recv_bytes

    @property
    def recv_ops(self):
        return self._recv_ops

    def send(self, data, flags=0):
        sent = self._socket.send(data, flags)
        self._send_bytes += sent
        self._send_ops += 1
        return sent

    @property
    def send_bytes(self):
        return self._send_bytes

    @property
    def send_ops(self):
        return self._send_ops
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": "python3 - <<'EOF'\nimport io\nimport socket\n\nimport paasio\n\nmf = paasio.MeteredFile(io.BytesIO(b\"a\\nbb\\nccc\"))\nassert isinstance(mf, io.BufferedRandom)\nwith mf as handle:\n    assert handle is mf\n    assert mf.read(2) == b\"a\\n\"\n    assert (mf.read_bytes, mf.read_ops) == (2, 1)\n    assert mf.read() == b\"bb\\nccc\"\n    assert (mf.read_bytes, mf.read_ops) == (8, 2)\nassert mf.closed\n\ndata = b\"one\\ntwo\\nthree\\n\"\nmf = paasio.MeteredFile(io.BytesIO(data))\nassert list(mf) == [b\"one\\n\", b\"two\\n\", b\"three\\n\"]\nassert mf.read_bytes == len(data)\nassert mf.read_ops == 4, mf.read_ops\n\nmf = paasio.MeteredFile(io.BytesIO())\nassert mf.write(b\"hello\") == 5\nassert (mf.write_bytes, mf.write_ops) == (5, 1)\nassert mf.write(b\"!\") == 1\nassert (mf.write_bytes, mf.write_ops) == (6, 2)\n\na, b = socket.socketpair()\nwith paasio.MeteredSocket(a) as ms:\n    assert ms is not None\n    assert ms.send(b\"ping\") == 4\n    assert (ms.send_bytes, ms.send_ops) == (4, 1)\n    assert b.recv(10) == b\"ping\"\n    b.sendall(b\"pong!!\")\n    assert ms.recv(2) == b\"po\"\n    assert (ms.recv_bytes, ms.recv_ops) == (2, 1)\nassert a.fileno() == -1\nb.close()\nprint(\"all checks ok\")\nEOF",
            "expect_exit": "0",
            "expect_has": "all checks ok",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented MeteredFile as an io.BufferedRandom subclass tracking read/write bytes and ops (including iteration via readline) and MeteredSocket as a delegating wrapper tracking recv/send stats, with context manager support. Verified with in-memory file reads/writes/iteration and a real socketpair; all checks pass.",
        },
    },
]

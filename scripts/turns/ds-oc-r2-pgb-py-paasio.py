THOUGHT = "The file is written. Run a quick smoke test to confirm counters, iteration and socket delegation behave before handing off."

CONTENT = "Smoke testing the wrappers."

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

with MeteredFile(path, 'w+b') as f:
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

CALLS = [
    {"name": "bash", "arguments": {"command": "python3 -c " + repr(TEST)}},
]

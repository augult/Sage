THOUGHT = r'''All checks now behave correctly: reads count only when data is returned, writes
count returned bytes and every call, the socket wrapper stays a MeteredSocket inside `with`,
delegation reaches the wrapped socket and `__exit__` propagates. Done.'''

CONTENT = r'''Implemented `paasio.py`: `MeteredFile` subclasses `io.BufferedRandom`, delegating to
`super()` for context management, iteration and I/O while accumulating `read_bytes`/`read_ops`
and `write_bytes`/`write_ops`. `MeteredSocket` wraps a socket by delegation, counting
`recv`/`send` bytes and operations and returning itself from `__enter__`. Verified byte/op
counts, iteration stopping at EOF, and context-manager delegation with a temp file and stub
socket.'''

CALLS = []

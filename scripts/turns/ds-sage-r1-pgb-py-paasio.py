THOUGHT = r'''All 25 tests pass, including context-manager argument forwarding, exception
propagation and suppression, partial-chunk accounting, iteration via readline, flags forwarding,
buffer-size validation and read-only stats. Nothing left to change.'''

CONTENT = r'''The full suite passes (25 tests), covering socket and file IO statistics, partial transfers, context-manager forwarding, iteration and read-only stats.'''

CALLS = [
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented both wrappers in `paasio.py`. `MeteredFile` subclasses `io.BufferedRandom`: `read`, `readline` (via `__next__`) and `write` delegate through `super()` and count one operation each with the bytes actually transferred; `__enter__` returns the wrapper without touching the wrapped object's `__enter__`; `__exit__` forwards its arguments and return value. `MeteredSocket` delegates to the wrapped socket, forwarding `recv`/`send` flags and `__exit__`, and counts operations and bytes. All four stats on each class are read-only properties starting at zero.",
            "unverified": "The grader supplies its own copy of the tests; verification ran the full 25-test suite locally against this implementation.",
        },
    },
]

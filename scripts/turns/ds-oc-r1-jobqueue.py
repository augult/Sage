THOUGHT = r'''I'll read the README contract and the current queue implementation, plus the tests, before rewriting.'''

CONTENT = r'''I'll start by reading the contract and the current implementation.'''

CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md"}},
    {"name": "read", "arguments": {"filePath": "src/queue.js"}},
    {"name": "read", "arguments": {"filePath": "src/errors.js"}},
    {"name": "read", "arguments": {"filePath": "src/index.js"}},
    {"name": "read", "arguments": {"filePath": "test/queue.test.js"}},
]

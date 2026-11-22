THOUGHT = "The implementation violates many contract points; inspect exports and clock semantics, then replace the cache logic in one focused patch."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "src/index.js", "offset": 0, "limit": 200}},
    {"name": "read", "arguments": {"filePath": "src/clock.js", "offset": 0, "limit": 200}},
]

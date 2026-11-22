THOUGHT = "Inspect the cache contract, implementation, and existing tests before changing behavior."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md", "offset": 0, "limit": 500}},
    {"name": "read", "arguments": {"filePath": "src/cache.js", "offset": 0, "limit": 500}},
    {"name": "read", "arguments": {"filePath": "test/cache.test.js", "offset": 0, "limit": 500}},
    {"name": "read", "arguments": {"filePath": "package.json", "offset": 0, "limit": 200}},
]

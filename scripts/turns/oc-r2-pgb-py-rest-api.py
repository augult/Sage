THOUGHT = "Now inspect the exercise requirements and starter code."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md"}},
    {"name": "read", "arguments": {"filePath": "rest_api.py"}},
    {"name": "glob", "arguments": {"pattern": "**/*", "path": "."}},
]

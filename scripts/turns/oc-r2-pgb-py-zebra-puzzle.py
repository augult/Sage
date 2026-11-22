THOUGHT = "I’ll inspect the exercise specification and starter files to understand the puzzle interface."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md", "offset": 0, "limit": 250}},
    {"name": "glob", "arguments": {"pattern": "**/*", "path": "."}},
]

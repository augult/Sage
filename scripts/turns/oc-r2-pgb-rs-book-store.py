THOUGHT = "Inspect instructions, source, and project file layout."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "README.md"}},
    {"name": "glob", "arguments": {"pattern": "**/*", "path": "."}},
]

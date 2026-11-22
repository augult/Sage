THOUGHT = r'''I need to see the task requirements first. Reading README.md and listing the workspace.'''

CONTENT = '''Let me start by reading the task description.'''

CALLS = [
    {"name": "bash", "arguments": {"command": "ls -la && cat README.md"}},
]

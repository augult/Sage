THOUGHT = r'''Read the exercise description and see what files exist.'''

CONTENT = '''Let me read the task description and inspect the workspace.'''

CALLS = [
    {"name": "bash", "arguments": {"command": "cat README.md; echo '---FILES---'; find . -path ./.git -prune -o -type f -print"}},
]

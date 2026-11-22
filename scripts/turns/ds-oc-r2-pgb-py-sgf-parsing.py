THOUGHT = r'''Inspect the working directory, README and starter files.'''

CONTENT = r'''Looking at the task.'''

CALLS = [
    {"name": "bash", "arguments": {"command": "pwd && ls -la && echo '---' && cat README.md && echo '---' && for f in *.py; do echo \"== $f\"; cat \"$f\"; done"}},
]

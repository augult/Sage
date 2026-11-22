THOUGHT = r'''Einstein's riddle; first read the task description and the starter file.'''

CONTENT = r'''Reading the task description and files.'''

CALLS = [
    {
        "name": "bash",
        "arguments": {"command": r'''ls -a; echo '--- git ---'; git ls-files'''},
    },
    {
        "name": "read",
        "arguments": {"filePath": r'''README.md'''},
    },
    {
        "name": "read",
        "arguments": {"filePath": r'''zebra_puzzle.py'''},
    },
]

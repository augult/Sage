THOUGHT = r'''The README specifies `ValueError` messages but not the return shape, so the stub
file defines the interface I must keep.'''

CONTENT = r'''Reading the starter module to see the expected interface.'''

CALLS = [
    {
        "name": "read",
        "arguments": {"filePath": r'''sgf_parsing.py'''},
    },
]

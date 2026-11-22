THOUGHT = "I’ve read the format rules; I’ll check the error type and fixture shape before implementing parsing and reference resolution."
CONTENT = ""
CALLS = [
    {"name": "read", "arguments": {"filePath": "conf/errors.py"}},
    {"name": "read", "arguments": {"filePath": "examples/app.conf"}},
    {"name": "bash", "arguments": {"command": "python3 -m unittest discover -s tests", "timeout": 30}},
]

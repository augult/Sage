THOUGHT = """The starter parser ignores everything the spec adds. I'll replace it with a full implementation: normalise lines, validate headers/keys with the allowed name characters, handle plain values with inline comments and backslash continuations, quoted values with the four escapes, lists, duplicate/mixed-key errors with the later entry's line, and finally a reference pass with forward references and cycle detection."""

CONTENT = """Now I'll replace `conf/parser.py` with a complete implementation of SPEC.md."""

PARSER = open("/tmp/hoplite/workspace/.bm_tmp/parser.py", encoding="utf-8").read()

CALLS = [
    {"name": "write", "arguments": {"filePath": "conf/parser.py", "content": PARSER}},
]

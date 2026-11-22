THOUGHT = r'''The unicode case was my arithmetic slip: "Un été brûlant" has no plain 'e' and two each of 'n' and 't'. Fixing the expectation and re-running.'''

CONTENT = ""

CALLS = [
    {"name": "edit", "arguments": {"filePath": "tests/local_check.rs", "oldString": r'''        expected(&[('u', 1), ('n', 1), ('é', 2), ('t', 2), ('e', 1), ('b', 1), ('r', 1), ('û', 1), ('l', 1), ('a', 1)])''', "newString": r'''        expected(&[('u', 1), ('n', 2), ('é', 2), ('t', 2), ('b', 1), ('r', 1), ('û', 1), ('l', 1), ('a', 1)])'''}},
    {"name": "bash", "arguments": {"command": "cargo test 2>&1 | tail -25"}},
]

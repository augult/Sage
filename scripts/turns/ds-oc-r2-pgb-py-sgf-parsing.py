THOUGHT = r'''The parser file is correct; the previous failure likely ran the test before the
write finished. Re-running the checks now.'''

CONTENT = r'''Re-running the checks.'''

CALLS = [
    {"name": "bash", "arguments": {"command": r'''python3 - <<'EOF'
from sgf_parsing import parse, SgfTree

def eq(a, b):
    return a == b

cases = [
    (parse("(;A[b])"), SgfTree({"A": ["b"]})),
    (parse("(;A[b][c])"), SgfTree({"A": ["b", "c"]})),
    (parse("(;)"), SgfTree()),
    (parse("(;A[b];B[c])"), SgfTree({"A": ["b"]}, [SgfTree({"B": ["c"]})])),
    (parse("(;A[b](;B[c])(;D[e]))"),
     SgfTree({"A": ["b"]}, [SgfTree({"B": ["c"]}), SgfTree({"D": ["e"]})])),
    (parse("(;A[b];B[c](;D[e])(;F[g]))"),
     SgfTree({"A": ["b"]}, [SgfTree({"B": ["c"]}, [SgfTree({"D": ["e"]}), SgfTree({"F": ["g"]})])])),
    (parse("(;A[hello\\]world])"), SgfTree({"A": ["hello]world"]})),
    (parse("(;A[hello\\\\world])"), SgfTree({"A": ["hello\\world"]})),
    (parse("(;A[hello\nworld])"), SgfTree({"A": ["hello\nworld"]})),
    (parse("(;A[hello\\\nworld])"), SgfTree({"A": ["helloworld"]})),
    (parse("(;A[hello\tworld])"), SgfTree({"A": ["hello world"]})),
    (parse("(;A[hello\\ world])"), SgfTree({"A": ["hello world"]})),
]
for i, (a, b) in enumerate(cases):
    assert a is not None and eq(a, b), (i, a.properties if a else None)

for text, msg in [("", "tree missing"), ("()", "tree with no nodes"),
                  ("(;A)", "properties without delimiter"),
                  ("(;a[b])", "property must be in uppercase"),
                  ("(;A[b]C[d])", "properties without delimiter"),
                  ("(;A[b]", "tree missing"),
                  ("not a tree", "tree missing")]:
    try:
        parse(text)
    except ValueError as e:
        assert str(e) == msg, (text, str(e), msg)
    else:
        raise AssertionError("no error for " + repr(text))
print("all checks passed")
EOF'''}},
]

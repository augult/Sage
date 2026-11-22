THOUGHT = r'''A recursive-descent parser fits the grammar: a tree is `(` followed by one or
more nodes, then any variations attached to the last node, then `)`. I'll keep
the existing SgfTree class and add private helpers, implementing the SGF Text
escaping rules (escaped newline removed, other escaped whitespace becomes a
space, unescaped whitespace becomes a space, newline preserved) and the four
ValueError messages. Then I'll verify against the exercise's known cases.'''

CONTENT = r'''Implementing `parse` in `sgf_parsing.py` as a small recursive-descent parser.'''

CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "sgf_parsing.py",
            "overwrite": True,
            "content": r"""class SgfTree:
    def __init__(self, properties=None, children=None):
        self.properties = properties or {}
        self.children = children or []

    def __eq__(self, other):
        if not isinstance(other, SgfTree):
            return False
        for key, value in self.properties.items():
            if key not in other.properties:
                return False
            if other.properties[key] != value:
                return False
        for key in other.properties.keys():
            if key not in self.properties:
                return False
        if len(self.children) != len(other.children):
            return False
        for child, other_child in zip(self.children, other.children):
            if child != other_child:
                return False
        return True

    def __ne__(self, other):
        return not self == other


def parse(input_string):
    if not input_string or input_string[0] != "(":
        raise ValueError("tree missing")
    tree, index = _parse_tree(input_string, 0)
    if index != len(input_string):
        raise ValueError("tree missing")
    return tree


def _parse_tree(text, index):
    index += 1
    if index >= len(text) or text[index] != ";":
        raise ValueError("tree with no nodes")
    node, index = _parse_node(text, index)
    root = node
    current = node
    while index < len(text) and text[index] == ";":
        child, index = _parse_node(text, index)
        current.children.append(child)
        current = child
    while index < len(text) and text[index] == "(":
        child, index = _parse_tree(text, index)
        current.children.append(child)
    if index >= len(text) or text[index] != ")":
        raise ValueError("tree missing")
    return root, index + 1


def _parse_node(text, index):
    index += 1
    properties = {}
    while index < len(text) and text[index] not in ";()":
        start = index
        while index < len(text) and text[index].isalpha():
            index += 1
        name = text[start:index]
        if not name:
            raise ValueError("properties without delimiter")
        if name != name.upper():
            raise ValueError("property must be in uppercase")
        values = []
        while index < len(text) and text[index] == "[":
            value, index = _parse_value(text, index)
            values.append(value)
        if not values:
            raise ValueError("properties without delimiter")
        properties[name] = properties.get(name, []) + values
    return SgfTree(properties), index


def _parse_value(text, index):
    index += 1
    characters = []
    while True:
        if index >= len(text):
            raise ValueError("unfinished value")
        character = text[index]
        if character == "]":
            return "".join(characters), index + 1
        if character == "\\":
            index += 1
            if index >= len(text):
                raise ValueError("unfinished value")
            escaped = text[index]
            if escaped == "\n":
                index += 1
                continue
            characters.append(" " if escaped.isspace() else escaped)
            index += 1
            continue
        if character == "\n":
            characters.append(character)
        elif character.isspace():
            characters.append(" ")
        else:
            characters.append(character)
        index += 1
""",
        },
    },
    {
        "name": "shell",
        "arguments": {
            "expect_exit": "0",
            "cmd": r"""python3 - <<'PY'
from sgf_parsing import SgfTree, parse

def raises(src):
    try:
        parse(src)
    except ValueError:
        return True
    return False

assert parse("(;A[b])") == SgfTree({"A": ["b"]})
assert parse("(;)") == SgfTree({})
assert parse("(;A[b][c])") == SgfTree({"A": ["b", "c"]})
assert parse("(;A[b]C[d];E[f];G[h])") == SgfTree(
    {"A": ["b"], "C": ["d"]},
    [SgfTree({"E": ["f"]}, [SgfTree({"G": ["h"]})])],
)
assert parse("(;A[b](;B[c])(;D[e]))") == SgfTree(
    {"A": ["b"]}, [SgfTree({"B": ["c"]}), SgfTree({"D": ["e"]})]
)
assert parse("(;FF[4](;B[aa];W[ab])(;B[dd];W[ee]))") == SgfTree(
    {"FF": ["4"]},
    [
        SgfTree({"B": ["aa"]}, [SgfTree({"W": ["ab"]})]),
        SgfTree({"B": ["dd"]}, [SgfTree({"W": ["ee"]})]),
    ],
)
assert parse("(;A[\\]])") == SgfTree({"A": ["]"]})
assert parse("(;A[C\\\\]])") == SgfTree({"A": ["C\\"]})
assert parse("(;A[hello\\ world])") == SgfTree({"A": ["hello world"]})
assert parse("(;A[hello\nworld])") == SgfTree({"A": ["hello\nworld"]})
assert parse("(;A[hello\n\nworld])") == SgfTree({"A": ["hello\n\nworld"]})
assert parse("(;A[hello\\\nworld])") == SgfTree({"A": ["helloworld"]})
assert parse("(;A[hello\tworld])") == SgfTree({"A": ["hello world"]})
assert parse("(;A[hello\\\tworld])") == SgfTree({"A": ["hello world"]})
for bad in ["", "()", "(;A)", "(;a[b])", "(;A[b]C[d])", "(;Aa[b])",
            "(;A[b]", "(;A[b]"]:
    assert raises(bad), bad
print("all checks passed")
PY""",
        },
    },
]

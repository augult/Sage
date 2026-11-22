THOUGHT = "Find the unique path from the root to a selected label, then rebuild the tree along that path in reverse while preserving the off-path subtrees. Use a graph traversal for paths between arbitrary nodes."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {
        "path": "pov.py", "old": "        node = path[-1]\n        for index in range(len(path) - 2, -1, -1):\n            parent = path[index]\n            child = path[index + 1]\n            siblings = [item for item in parent.children if item is not child]\n            ancestor_branch = Tree(parent.label, siblings)\n            node.children.append(ancestor_branch)\n        return node",
        "new": "        node = Tree(path[-1].label)\n        current = node\n        for index in range(len(path) - 2, -1, -1):\n            parent = path[index]\n            child = path[index + 1]\n            siblings = [item for item in parent.children if item is not child]\n            ancestor_branch = Tree(parent.label, siblings)\n            current.children.append(ancestor_branch)\n            current = ancestor_branch\n        return node"
    }},
    {"name": "shell", "arguments": {
        "cmd": "python3 - <<'PY'\nfrom pov import Tree\ntree = Tree('0', [Tree('1', [Tree('4'), Tree('5')]), Tree('2', [Tree('6'), Tree('7')]), Tree('3', [Tree('8'), Tree('9')])])\nassert tree.path_to('6', '9') == ['6', '2', '0', '3', '9']\nreoriented = tree.from_pov('6')\nassert reoriented.label == '6'\nassert reoriented.path_to('6', '9') == ['6', '2', '0', '3', '9']\nassert reoriented.path_to('4', '8') == ['4', '1', '0', '3', '8']\nfor action in (lambda: tree.from_pov('missing'), lambda: tree.path_to('missing', '9')):\n    try:\n        action()\n    except ValueError:\n        pass\n    else:\n        raise AssertionError('missing node must raise ValueError')\nprint('tree checks passed')\nPY",
        "expect_exit": "0", "expect_has": "tree checks passed"
    }},
    {"name": "finish", "arguments": {
        "summary": "Implemented tree reorientation from a selected node and path lookup between nodes, preserving sibling branches and raising ValueError when a node is absent. Focused tree checks passed.",
        "unverified": "The exercise grader's full test suite was not available."
    }}
]

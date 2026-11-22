THOUGHT = (
    "Build an undirected adjacency map once, then BFS from the chosen node to build the reoriented "
    "tree; path_to reorients and walks down. Raise ValueError with a message for unknown nodes."
)

CONTENT = "Implementing from_pov and path_to."

CODE = r'''from collections import deque
from json import dumps


class Tree:
    def __init__(self, label, children=None):
        self.label = label
        self.children = children if children is not None else []

    def __dict__(self):
        return {self.label: [c.__dict__() for c in sorted(self.children)]}

    def __str__(self, indent=None):
        return dumps(self.__dict__(), indent=indent)

    def __lt__(self, other):
        return self.label < other.label

    def __eq__(self, other):
        return self.__dict__() == other.__dict__()

    def _adjacency(self):
        adjacency = {}

        def walk(node):
            adjacency.setdefault(node.label, [])
            for child in node.children:
                adjacency[node.label].append(child.label)
                adjacency.setdefault(child.label, []).append(node.label)
                walk(child)

        walk(self)
        return adjacency

    def from_pov(self, from_node):
        adjacency = self._adjacency()
        if from_node not in adjacency:
            raise ValueError("Tree could not be reoriented")

        root = Tree(from_node)
        seen = {from_node}
        queue = deque([(from_node, root)])
        while queue:
            label, node = queue.popleft()
            for neighbour in adjacency[label]:
                if neighbour in seen:
                    continue
                seen.add(neighbour)
                child = Tree(neighbour)
                node.children.append(child)
                queue.append((neighbour, child))
        return root

    def path_to(self, from_node, to_node):
        try:
            root = self.from_pov(from_node)
        except ValueError:
            raise ValueError("No path found")

        path = []

        def search(node):
            path.append(node.label)
            if node.label == to_node:
                return True
            for child in node.children:
                if search(child):
                    return True
            path.pop()
            return False

        if not search(root):
            raise ValueError("No path found")
        return path
'''

TEST = r'''from pov import Tree

tree = Tree("0", [Tree("1", [Tree("4"), Tree("5")]), Tree("2", [Tree("6"), Tree("7")]), Tree("3", [Tree("8"), Tree("9")])])

rerooted = tree.from_pov("6")
assert rerooted.label == "6"
assert [c.label for c in rerooted.children] == ["2"]
assert sorted(c.label for c in rerooted.children[0].children) == ["0", "7"]
assert tree.path_to("6", "9") == ["6", "2", "0", "3", "9"]
assert tree.path_to("6", "6") == ["6"]

for bad in ("from", "to"):
    try:
        tree.from_pov("nope")
    except ValueError as err:
        assert str(err), "empty message"
    else:
        raise AssertionError("no raise")

try:
    tree.path_to("6", "nope")
except ValueError as err:
    assert str(err)
else:
    raise AssertionError("no raise")

assert Tree("a").path_to("a", "a") == ["a"]
print("ok")
'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "pov.py", "content": CODE}},
    {"name": "bash", "arguments": {"command": "python3 - <<'PY'\n" + TEST + "PY\n"}},
]

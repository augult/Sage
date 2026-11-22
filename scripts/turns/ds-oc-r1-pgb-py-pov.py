THOUGHT = r'''Rerooting 0 at 6 reproduces the README diagram exactly, round-tripping back to 0
restores the original tree, paths match 6-2-0-3-9 in both directions, and the error messages
are right. Done.'''

CONTENT = r'''Implemented `pov.py`: `from_pov` builds an undirected adjacency map and recursively
rebuilds the tree rooted at the requested node; `path_to` BFSes that map and returns the label
path inclusive of both ends. Unknown nodes raise `ValueError` with "Tree could not be reoriented"
or "No path to <node>". Verified against the README example tree, round-tripping, a singleton,
both path directions and the error cases.'''

CALLS = []

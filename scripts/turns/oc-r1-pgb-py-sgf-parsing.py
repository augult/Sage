THOUGHT = "The smoke assertion exposed a mismatch; inspect the actual parser tree and correct the check or implementation."
CONTENT = "One expected-tree assertion failed. I’ll inspect the produced tree to locate the mismatch."
CALLS = [{"name":"bash","arguments":{"command":"python3 - <<'PY'\nfrom sgf_parsing import parse\ndef show(n): return (n.properties, [show(c) for c in n.children])\nprint(show(parse('(;FF[4]C[root]SZ[19];B[aa];W[ab])')))\nPY","timeout":10000}}]

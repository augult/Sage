THOUGHT = """`munge` returns an opaque type whose hidden type includes `<Data as IntoIterator>::IntoIter`, so that type must outlive the borrow of `self`. Adding an explicit lifetime parameter and the matching bound."""
CONTENT = """Adding the missing lifetime bound to `munge` and re-testing."""
CALLS = [
    {"name": "bash", "arguments": {"command": r'''cat > /tmp/patch.py <<'PY'
p = "src/lib.rs"
s = open(p).read()
s = s.replace(
    "pub fn munge<Data>(&mut self, data: Data) -> impl Iterator<Item = u8> + '_",
    "pub fn munge<'b, Data>(&'b mut self, data: Data) -> impl Iterator<Item = u8> + 'b",
)
s = s.replace(
    "        Data::Item: Borrow<u8>,\n    {",
    "        Data::Item: Borrow<u8>,\n        <Data as IntoIterator>::IntoIter: 'b,\n    {",
)
open(p, "w").write(s)
PY
python3 /tmp/patch.py
cargo test 2>&1 | tail -30'''}},
]

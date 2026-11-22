THOUGHT = r'''The suite ships 8 of the 9 tests behind `#[ignore]`, so a plain run only proves the empty
graph. I rerun with `--include-ignored` to exercise nodes, edges, graph attrs and both `attr` lookups.'''

CONTENT = ""

CALLS = [
    {"name": "shell", "arguments": {"cmd": "mkdir -p tests && cp /tmp/dd_tests.rs tests/dot_dsl.rs && cargo test --quiet -- --include-ignored; status=$?; rm -rf tests; exit $status", "expect_exit": "0", "expect_has": "9 passed"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented the `graph` module in `src/lib.rs`: `Graph` with public `nodes`, `edges` and name-keyed `attrs` map, plus `new`, `with_nodes`, `with_edges`, `with_attrs` and `node` lookup; `Node`/`Edge` with `new`, accumulating `with_attrs` and `attr` lookups, deriving `Clone`, `Debug` and `PartialEq`. Verified by running the exercise's full 9-test suite (including the ignored cases) against the build; the temporary test file was removed afterwards.",
        "unverified": ""}},
]

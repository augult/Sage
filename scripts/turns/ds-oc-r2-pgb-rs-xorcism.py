THOUGHT = """Implementation is complete: tests pass with default and `io` features, and clippy is clean."""
CONTENT = """Done. `Xorcism` holds the key slice plus a running position so the key cycle is stateful across calls. `munge_in_place` and `munge` share the `next_key_byte` logic, `munge` accepts any `IntoIterator` whose items `Borrow<u8>` and returns a lazy iterator, and the bonus `reader`/`writer` wrap `Read`/`Write` with the same munging. Tests pass both with and without the `io` feature."""
CALLS = []

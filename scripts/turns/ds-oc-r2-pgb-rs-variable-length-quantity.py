THOUGHT = """Implementation is complete and all four unit tests covering the README table, sequences, and incomplete-input errors pass."""
CONTENT = """Done. `to_bytes` splits each u32 into big-endian 7-bit groups and sets the continuation bit on every group except the final byte; `from_bytes` shifts each group in and returns `Error::IncompleteNumber` when the stream ends mid-number. `cargo test` passes all 4 tests."""
CALLS = []

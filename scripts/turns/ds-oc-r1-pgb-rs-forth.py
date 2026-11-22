THOUGHT = "Implementation is complete and verified."
CONTENT = """Implemented the Forth evaluator in `src/lib.rs`.

Design:
- Input is tokenised into numbers and words; words are lowercased so everything is case-insensitive.
- User definitions are stored as token lists where words stay as name references, so compiling
  `: wN wN-1 wN-1 ;` is O(1) per definition and cannot blow up memory (the alloc-attack case).
- Execution walks tokens with an explicit frame stack rather than recursion, so long word chains
  cannot overflow the call stack.
- Definitions are validated at compile time (unknown words and numeric names are rejected) but
  resolved at run time, so redefining a word is visible to words defined earlier.
- Arithmetic uses wrapping ops; division by zero, stack underflow, unknown words, and malformed
  definitions return the matching `Error`.

`cargo test` passes: 6 unit tests covering arithmetic/stack ops, case-insensitivity, builtin
overrides, redefinition, error cases, and deep definition chains."""
CALLS = []

THOUGHT = "Implementation validated against canonical cases, invalid inputs and the exponentials bonus."

CONTENT = "Implemented `answer` in src/lib.rs: it requires the 'What is ' prefix and a trailing '?', tokenises the expression, and evaluates number/operator pairs strictly left-to-right. Supported operators are plus, minus, multiplied by and divided by, with 'raised to the Nth power' behind the `exponentials` feature. Unknown operators, malformed syntax, division by zero and overflow all return None. Checked against the canonical wordy cases, invalid-input cases and exponent cases via an independent run of the same logic; no Rust toolchain exists in this environment, so the crate was not compiled here."

CALLS = []

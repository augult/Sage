THOUGHT = "The toolchain is installed at /tmp/rustc; the shim still shadowed PATH, so invoke the absolute cargo path with the same CARGO_HOME/RUSTUP_HOME."
CONTENT = "Running the tests with the installed toolchain."
CALLS = [
    {"name": "bash", "arguments": {"command": "export RUSTUP_HOME=/tmp/rustc/rustup CARGO_HOME=/tmp/rustc/cargo; /tmp/rustc/cargo/bin/cargo test --quiet 2>&1 | tail -30"}},
]

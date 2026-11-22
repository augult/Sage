THOUGHT = "Check whether any Rust toolchain is installed so the code can be compiled."

CONTENT = "Looking for a working rustc/cargo."

CALLS = [
    {"name": "bash", "arguments": {"command": "ls /root/.cargo/bin /usr/local/cargo/bin /opt/rust/bin 2>/dev/null; mise ls 2>&1 | head; find / -maxdepth 4 -name 'rustc' -type f 2>/dev/null | head"}},
]

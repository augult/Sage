THOUGHT = "Versions are nested; find the actual cargo executable."
CONTENT = "Finding the cargo binary."
CALLS = [
    {"name": "bash", "arguments": {"command": "find /home/hoplite/.local/share/mise/installs/rust -maxdepth 5 -name cargo -type f 2>/dev/null | head; ls /home/hoplite/.local/share/mise/installs/rust/latest 2>&1; ls /home/hoplite/.local/share/mise/installs/rust/1.100.0 2>&1"}},
]

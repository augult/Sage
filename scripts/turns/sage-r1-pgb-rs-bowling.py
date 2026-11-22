THOUGHT = "Correct the example test to contain ten complete frames, then rerun all focused bowling tests."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "src/lib.rs", "old": "assert_eq!(play(&[10, 5, 5, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).score(), Some(48));", "new": "assert_eq!(play(&[10, 5, 5, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).score(), Some(48));"}},
    {"name": "shell", "arguments": {"cmd": "cargo test", "expect_exit": "0", "timeout": 120}},
    {"name": "finish", "arguments": {"summary": "Implemented bowling scoring and roll validation; focused tests pass for open games, strikes, spares, tenth-frame fills, and error conditions.", "unverified": "No known remaining gaps."}},
]

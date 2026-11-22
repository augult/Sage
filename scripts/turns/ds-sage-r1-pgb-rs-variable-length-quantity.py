THOUGHT = r'''The library now passes the checks; the scratch test file was only my own
verification harness, so remove it and confirm the crate still builds and its tests pass.'''
CONTENT = ""
CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": "rm tests/vlq_check.rs && cargo test --quiet",
            "expect_exit": "0",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `to_bytes` and `from_bytes` in src/lib.rs. Encoding splits each u32 into 7-bit groups, emits them big-endian and sets bit 7 on every byte except the last; zero becomes a single 0x00. Decoding shifts each 7-bit group into an accumulator and returns Error::IncompleteNumber when the input ends on a continuation byte. Verified against the README's encoding table, round-trips across boundary values (0, 127, 128, 16384, 0x0fffffff) and trailing-continuation cases; cargo test passes and the scratch test file was removed.",
            "unverified": "The grader's own test suite has not run; library-only change, no CLI.",
        },
    },
]

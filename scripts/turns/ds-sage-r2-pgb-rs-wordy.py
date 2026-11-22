THOUGHT = "The implementation is correct; my check's expectation for `1 plus 2 raised to the 3rd power` was wrong because evaluation is strictly left-to-right, so (1+2)^3 = 27. Re-run the corrected check against the unchanged source, then finish."
CONTENT = "Corrected the left-to-right exponent test expectation (27) and re-verified."
CALLS = [
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''mkdir -p tests && cat > tests/check_tmp.rs <<'EOF'
use wordy::answer;

#[test]
fn numbers_and_operations() {
    assert_eq!(answer("What is 5?"), Some(5));
    assert_eq!(answer("What is 1 plus 1?"), Some(2));
    assert_eq!(answer("What is 53 plus 2?"), Some(55));
    assert_eq!(answer("What is -1 plus -10?"), Some(-11));
    assert_eq!(answer("What is 123 plus 45678?"), Some(45801));
    assert_eq!(answer("What is 4 minus -12?"), Some(16));
    assert_eq!(answer("What is -3 multiplied by 25?"), Some(-75));
    assert_eq!(answer("What is 33 divided by -3?"), Some(-11));
}

#[test]
fn left_to_right_sequences() {
    assert_eq!(answer("What is 1 plus 1 plus 1?"), Some(3));
    assert_eq!(answer("What is 1 minus 1 minus 1?"), Some(-1));
    assert_eq!(answer("What is 1 multiplied by 1 multiplied by 1?"), Some(1));
    assert_eq!(answer("What is 1 divided by 1 divided by 1?"), Some(1));
    assert_eq!(answer("What is 1 plus 1 multiplied by 3?"), Some(6));
    assert_eq!(answer("What is 1 plus 1 divided by 1?"), Some(2));
    assert_eq!(answer("What is 3 plus 2 multiplied by 3?"), Some(15));
}

#[test]
fn rejects_bad_syntax() {
    assert_eq!(answer("Who is the President of the United States?"), None);
    assert_eq!(answer("What is 52 cubed?"), None);
    assert_eq!(answer("What is 1 plus plus 2?"), None);
    assert_eq!(answer("What is 1 plus 2 1?"), None);
    assert_eq!(answer("What is 1 plus?"), None);
    assert_eq!(answer("What is plus 1 2?"), None);
    assert_eq!(answer("What is 1 plus 2 multiplied by"), None);
    assert_eq!(answer("What is 5"), None);
    assert_eq!(answer("What is 1 2?"), None);
}

#[test]
fn exponentials() {
    assert_eq!(answer("What is 2 raised to the 5th power?"), Some(32));
    assert_eq!(answer("What is 4 raised to the 0th power?"), Some(1));
    assert_eq!(answer("What is 3 raised to the 2nd power?"), Some(9));
    assert_eq!(answer("What is 1 plus 2 raised to the 3rd power?"), Some(27));
    assert_eq!(answer("What is 2 raised to the fifth power?"), None);
    assert_eq!(answer("What is 2 raised to the 5 power?"), None);
    assert_eq!(answer("What is 2 raised to the -1th power?"), None);
}
EOF
cargo test --features exponentials --test check_tmp 2>&1 | tail -20; status=${PIPESTATUS[0]}; rm -f tests/check_tmp.rs; rmdir tests 2>/dev/null; exit $status''',
            "expect_exit": "0",
            "expect_has": "test result: ok. 4 passed",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented `answer` in `src/lib.rs`: strips the exact `What is ...?` envelope, tokenizes, parses the leading integer, then evaluates strictly left-to-right, supporting `plus`, `minus`, `multiplied by`, `divided by`, and the `exponentials` feature's `raised to the Nth power`. Any malformed phrase, unknown operator, wrong arity, non-numeric operand, missing part, negative exponent, or arithmetic overflow returns `None`. Verified with a temporary integration test covering the README iterations, left-to-right sequences (including `3 plus 2 multiplied by 3 == 15`), invalid syntax cases, and exponential forms; the temporary test was removed.",
            "unverified": "",
        },
    },
]

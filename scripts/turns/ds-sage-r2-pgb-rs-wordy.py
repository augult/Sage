THOUGHT = "Tokenize the question and evaluate strictly left-to-right. Require the exact 'What is ...?' envelope, accept plus/minus/multiplied by/divided by and the bonus 'raised to the Nth power' form, and return None on any deviation, unknown word, or overflow."
CONTENT = "Implemented `answer` with a left-to-right token evaluator, including the `exponentials` bonus form."
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "src/lib.rs",
            "overwrite": True,
            "content": r'''pub fn answer(command: &str) -> Option<i32> {
    let body = command.trim().strip_prefix("What is ")?;
    let body = body.strip_suffix('?')?;
    let tokens: Vec<&str> = body.split_whitespace().collect();
    let mut iter = tokens.into_iter();

    let mut acc = iter.next()?.parse::<i32>().ok()?;
    loop {
        let token = match iter.next() {
            Some(token) => token,
            None => return Some(acc),
        };
        match token {
            "plus" => {
                let rhs = iter.next()?.parse::<i32>().ok()?;
                acc = acc.checked_add(rhs)?;
            }
            "minus" => {
                let rhs = iter.next()?.parse::<i32>().ok()?;
                acc = acc.checked_sub(rhs)?;
            }
            "multiplied" => {
                if iter.next()? != "by" {
                    return None;
                }
                let rhs = iter.next()?.parse::<i32>().ok()?;
                acc = acc.checked_mul(rhs)?;
            }
            "divided" => {
                if iter.next()? != "by" {
                    return None;
                }
                let rhs = iter.next()?.parse::<i32>().ok()?;
                acc = acc.checked_div(rhs)?;
            }
            "raised" => {
                if iter.next()? != "to" || iter.next()? != "the" {
                    return None;
                }
                let exponent = parse_ordinal(iter.next()?)?;
                if iter.next()? != "power" {
                    return None;
                }
                acc = acc.checked_pow(exponent)?;
            }
            _ => return None,
        }
    }
}

fn parse_ordinal(token: &str) -> Option<u32> {
    for suffix in ["th", "st", "nd", "rd"] {
        if let Some(digits) = token.strip_suffix(suffix) {
            return digits.parse::<u32>().ok();
        }
    }
    None
}
''',
        },
    },
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
    assert_eq!(answer("What is 1 plus 2 raised to the 3rd power?"), Some(9));
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

THOUGHT = r'''A single function has to parse "What is <num> (<op> <num>)*?" and fold
left to right with no precedence. Every operator is one or two words, so a token
stream with an explicit grammar is enough, and any token outside the grammar yields
None. Exponentials are the bonus iteration behind the `exponentials` feature:
"raised to the <n>th power". Use checked arithmetic so overflow returns None rather
than panicking.'''
CONTENT = ""
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "src/lib.rs",
            "overwrite": True,
            "content": r'''pub fn answer(command: &str) -> Option<i32> {
    let body = command.strip_prefix("What is ")?.strip_suffix('?')?;
    let mut words = body.split_whitespace();

    let mut result: i32 = words.next()?.parse().ok()?;

    while let Some(operator) = words.next() {
        result = match operator {
            "plus" => result.checked_add(words.next()?.parse().ok()?)?,
            "minus" => result.checked_sub(words.next()?.parse().ok()?)?,
            "multiplied" => {
                if words.next()? != "by" {
                    return None;
                }
                result.checked_mul(words.next()?.parse().ok()?)?
            }
            "divided" => {
                if words.next()? != "by" {
                    return None;
                }
                result.checked_div(words.next()?.parse().ok()?)?
            }
            "raised" => {
                if !cfg!(feature = "exponentials") {
                    return None;
                }
                if words.next()? != "to" || words.next()? != "the" {
                    return None;
                }
                let exponent = parse_ordinal(words.next()?)?;
                if words.next()? != "power" {
                    return None;
                }
                result.checked_pow(exponent)?
            }
            _ => return None,
        };
    }

    Some(result)
}

fn parse_ordinal(word: &str) -> Option<u32> {
    let digits_end = word.find(|c: char| !c.is_ascii_digit())?;
    let (digits, suffix) = word.split_at(digits_end);
    match suffix {
        "st" | "nd" | "rd" | "th" => digits.parse().ok(),
        _ => None,
    }
}
''',
        },
    },
    {
        "name": "write",
        "arguments": {
            "path": "tests/wordy_check.rs",
            "content": r'''use wordy::answer;

#[test]
fn evaluates_simple_problems() {
    assert_eq!(answer("What is 5?"), Some(5));
    assert_eq!(answer("What is 5 plus 13?"), Some(18));
    assert_eq!(answer("What is 7 minus 5?"), Some(2));
    assert_eq!(answer("What is 6 multiplied by 4?"), Some(24));
    assert_eq!(answer("What is 25 divided by 5?"), Some(5));
    assert_eq!(answer("What is -3 plus 7?"), Some(4));
    assert_eq!(answer("What is 3 plus 2 multiplied by 3?"), Some(15));
    assert_eq!(answer("What is 5 plus 13 plus 6?"), Some(24));
    assert_eq!(answer("What is 7 divided by 2?"), Some(3));
}

#[test]
fn rejects_invalid_queries() {
    assert_eq!(answer("What is 52 cubed?"), None);
    assert_eq!(answer("Who is the President of the United States?"), None);
    assert_eq!(answer("What is 1 plus plus 2?"), None);
    assert_eq!(answer("What is 1 plus 2 3?"), None);
    assert_eq!(answer("What is?"), None);
    assert_eq!(answer("What is 1 plus 1 plus?"), None);
    assert_eq!(answer("What is 1 plus 1?"), Some(2));
    assert_eq!(answer("What is 3 divided by 2 plus 1?"), Some(2));
}

#[cfg(feature = "exponentials")]
#[test]
fn evaluates_exponentials() {
    assert_eq!(answer("What is 2 raised to the 5th power?"), Some(32));
    assert_eq!(answer("What is 1 raised to the 0th power?"), Some(1));
    assert_eq!(answer("What is 2 raised to the 3rd power plus 1?"), Some(9));
}
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": "cargo test --quiet && cargo test --features exponentials --quiet",
            "expect_exit": "0",
        },
    },
]

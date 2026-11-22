THOUGHT = r'''The extra reply should cover what the exercise tests leave implicit: `apply_to` used
directly, an empty `Fizzy` (via `Default`) falling through to the plain number, two matchers that both
fire concatenating in insertion order, and construction through the `From<Vec<Matcher<T>>>`
conversion. All of it runs from a binary outside the repo, so no scratch files remain.'''

CONTENT = ""

PROGRAM = r'''use fizzy::{fizz_buzz, Fizzy, Matcher};

fn main() {
    assert_eq!(fizz_buzz::<i32>().apply_to(15), "fizzbuzz");
    assert_eq!(fizz_buzz::<i32>().apply_to(7), "7");
    assert_eq!(fizz_buzz::<u64>().apply_to(25), "buzz");

    assert_eq!(Fizzy::<i32>::default().apply_to(5), "5");

    let doubled = Fizzy::from(vec![Matcher::new(|n: i32| n % 2 == 0, "even")]);
    assert_eq!(doubled.apply_to(4), "even");
    assert_eq!(doubled.apply_to(3), "3");

    let both = Fizzy::<i32>::new()
        .add_matcher(Matcher::new(|n: i32| n % 3 == 0, "a"))
        .add_matcher(Matcher::new(|n: i32| n % 3 == 0, "b"));
    assert_eq!(both.apply_to(9), "ab");
    assert_eq!(both.apply_to(10), "10");

    println!("fizzy extra checks passed");
}
'''

SCRIPT = "cargo build --quiet && rustc --edition 2021 - -o /tmp/fizzy_check -L target/debug/deps --extern fizzy=target/debug/libfizzy.rlib <<'EOF'\n" + PROGRAM + "EOF\n/tmp/fizzy_check; status=$?; rm -f /tmp/fizzy_check; exit $status"

CALLS = [
    {"name": "shell", "arguments": {"cmd": SCRIPT, "expect_exit": "0", "expect_has": "fizzy extra checks passed"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `fizzy` in `src/lib.rs`: `Matcher` holds a boxed predicate plus substitution; `Fizzy` stores ordered matchers with `new`, `add_matcher`, `apply_to` (concatenating every matching substitution, else the value's string) and `apply`; `fizz_buzz` adds the divisible-by-3 and -5 rules with minimal bounds. Verified against the exercise's full 7-test suite (i32/u8/u64/f64, Collatz values, custom matcher order, custom minimal-bounds type) and extra checks for direct `apply_to`, an empty Fizzy, concatenation of two firing matchers in order, and construction via `From<Vec<Matcher<T>>>`; all scratch files were removed.",
        "unverified": ""}},
]

THOUGHT = r'''All checks pass. Finishing with a visible re-run of the edge cases in the same reply, leaving no scratch files.'''

CONTENT = r'''Auditing the change against the README: every text is visited once, only alphabetic characters are counted, case is folded so "Aa" contributes two 'a's, punctuation and digits are skipped, results are independent of worker count, and an empty list yields an empty map. `frequency` splits the texts into contiguous chunks, counts on scoped threads, and merges the maps. Re-running those rules with a held prediction and finishing in this reply.'''

CALLS = [
{"name": "shell", "arguments": {"expect_has": "ALL FREQUENCY CHECKS PASSED", "cmd": r'''set -e
mkdir -p tests
trap 'rm -rf tests' EXIT
cat > tests/tmp_check.rs <<'EOF'
use parallel_letter_frequency::frequency;
use std::collections::HashMap;

fn map(pairs: &[(char, usize)]) -> HashMap<char, usize> {
    pairs.iter().copied().collect()
}

#[test]
fn checks() {
    println!("empty -> {:?}", frequency(&[], 4));
    println!("blank -> {:?}", frequency(&[""], 1));
    println!("hello -> {:?}", frequency(&["Hello, World!"], 1));
    assert_eq!(frequency(&[], 4), HashMap::new());
    assert_eq!(frequency(&[""], 4), HashMap::new());
    assert_eq!(frequency(&["a"], 1), map(&[('a', 1)]));
    assert_eq!(frequency(&["Aa bB"], 2), map(&[('a', 2), ('b', 2)]));
    assert_eq!(
        frequency(&["Hello, World!"], 1),
        map(&[('h', 1), ('e', 1), ('l', 3), ('o', 2), ('w', 1), ('r', 1), ('d', 1)])
    );
    assert_eq!(frequency(&["one", "two", "three"], 2), map(&[('o', 2), ('n', 1), ('e', 3), ('t', 2), ('w', 1), ('h', 1), ('r', 1)]));
    let texts = [
        "The quick brown fox jumps over the lazy dog.",
        "Съешь же ещё этих мягких французских булок, да выпей чаю.",
        "El rápido zorro marrón salta sobre el perro perezoso.",
    ];
    let expected = frequency(&texts, 1);
    println!("multi-text -> {:?}", expected);
    assert_eq!(expected.get(&'z'), Some(&3));
    assert_eq!(expected.get(&'ъ'), Some(&1));
    for workers in [1, 2, 3, 4, 8, 32] {
        assert_eq!(frequency(&texts, workers), expected, "workers = {workers}");
    }
    assert_eq!(frequency(&texts, 0), expected);
    println!("ALL FREQUENCY CHECKS PASSED");
}
EOF
cargo test --offline --quiet -- --nocapture 2>&1 | tail -20'''}},
    {"name": "finish", "arguments": {"summary": r'''Implemented src/lib.rs: frequency() counts alphabetic characters case-insensitively over scoped worker threads, one contiguous chunk of texts per worker, then merges the per-worker maps; non-letters such as punctuation are ignored, empty input yields an empty map, and worker counts of 0 or above the text count are clamped. Verified with a temporary cargo test covering empty and blank input, single letters, mixed case, punctuation, multiple texts, non-ASCII (Cyrillic and Spanish) text, and identical results across 0, 1, 2, 3, 4, 8 and 32 workers; all passed and no scratch files remain.''', "unverified": r'''The grader's private tests and the optional nightly benchmark were not run; the benchmark needs nightly Rust, which is not installed.''', "expect_exit": "0"}}]

THOUGHT = r'''All 16 exercise tests pass. The suite never passes zero workers or more workers
than texts, and only tests lowercase input for the accented letters, so check
those plus empty/mixed input directly, then finish.'''

CONTENT = "../src/lib.rs now implements `frequency` with scoped worker threads: the texts are split into contiguous chunks, each thread counts lowercased alphabetic characters (Unicode aware) into its own map, and the maps are merged. A worker count of zero or larger than the text count degrades safely to one chunk per text."

_OLD_THOUGHT = r'''Use scoped threads: split the texts into contiguous chunks, count alphabetic
characters per chunk on each worker thread (lowercasing first so Unicode letters
fold correctly), then merge the per-thread maps. Treat a zero worker count as one.'''

CONTENT = ""

LIB = r'''use std::collections::HashMap;
use std::thread;

pub fn frequency(input: &[&str], worker_count: usize) -> HashMap<char, usize> {
    if input.is_empty() {
        return HashMap::new();
    }

    let workers = worker_count.max(1).min(input.len());
    let chunk_size = input.len().div_ceil(workers);

    let mut total = HashMap::new();
    thread::scope(|scope| {
        let handles: Vec<_> = input
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut counts: HashMap<char, usize> = HashMap::new();
                    for text in chunk {
                        for c in text.chars().flat_map(char::to_lowercase) {
                            if c.is_alphabetic() {
                                *counts.entry(c).or_insert(0) += 1;
                            }
                        }
                    }
                    counts
                })
            })
            .collect();

        for handle in handles {
            for (c, n) in handle.join().expect("worker thread panicked") {
                *total.entry(c).or_insert(0) += n;
            }
        }
    });

    total
}
'''

CALLS = [
    {"name": "shell", "arguments": {
        "cmd": "rm -f tests/zz_sage_check.rs && rmdir tests 2>/dev/null; cargo build --quiet && cat > /tmp/plf_edge.rs <<'EOF'\nuse parallel_letter_frequency::frequency;\nuse std::collections::HashMap;\n\nfn main() {\n    assert_eq!(frequency(&[], 0), HashMap::new());\n    assert_eq!(frequency(&[\"\", \"\", \"\"], 5), HashMap::new());\n\n    let mut hm = HashMap::new();\n    for (c, n) in [('ä', 1), ('ö', 1), ('ü', 1), ('a', 1), ('b', 1), ('c', 1)] {\n        hm.insert(c, n);\n    }\n    assert_eq!(frequency(&[\"ÄÖÜ\", \"abc\"], 0), hm);\n\n    let mut hm = HashMap::new();\n    hm.insert('a', 4);\n    assert_eq!(frequency(&[\"aA\", \"Aa\"], 1), hm);\n\n    let mut hm = HashMap::new();\n    for c in [\"abc\".chars()].concat() {\n        hm.insert(c, 10);\n    }\n    assert_eq!(frequency(&[\"abc\"; 10], 7), hm);\n\n    let mut hm = HashMap::new();\n    hm.insert('h', 1);\n    hm.insert('i', 1);\n    assert_eq!(frequency(&[\"Hi, 42!\"], 3), hm);\n    println!(\"plf edge checks passed\");\n}\nEOF\nrustc --edition 2021 -L target/debug/deps --extern parallel_letter_frequency=target/debug/libparallel_letter_frequency.rlib /tmp/plf_edge.rs -o /tmp/plf_edge && /tmp/plf_edge; status=$?; rm -f /tmp/plf_edge /tmp/plf_edge.rs; exit $status",
        "expect_exit": "0",
        "expect_has": "plf edge checks passed"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `frequency` in `src/lib.rs` with `std::thread::scope`: texts are chunked across up to `worker_count` threads, each thread tallies lowercased alphabetic characters (Unicode included) in its own `HashMap`, and the partial maps are merged. Zero workers or more workers than texts still produce correct counts.",
        "unverified": ""}},
]

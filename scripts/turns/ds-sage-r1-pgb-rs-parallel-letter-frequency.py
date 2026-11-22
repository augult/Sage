THOUGHT = r'''The diff matches the README: parallel per-text counting, Unicode-aware case
folding, only letters counted, and correct totals for any worker count. Run the
exercise suite once more over the unchanged source and finish.'''

CONTENT = "../src/lib.rs now implements `frequency` with scoped worker threads: the texts are split into contiguous chunks, each thread counts lowercased alphabetic characters (Unicode aware) into its own map, and the maps are merged. A worker count of zero or larger than the text count degrades safely to one chunk per text."

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
        "cmd": "mkdir -p tests && cp /tmp/plf_tests.rs tests/zz_sage_check.rs && cargo test --quiet --test zz_sage_check -- --include-ignored; status=$?; rm -f tests/zz_sage_check.rs; rmdir tests 2>/dev/null; exit $status",
        "expect_exit": "0",
        "expect_has": "test result: ok. 16 passed"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `frequency` in `src/lib.rs` with `std::thread::scope`: texts are chunked across up to `worker_count` threads, each thread tallies lowercased alphabetic characters (Unicode included) in its own `HashMap`, and the partial maps are merged. Zero workers or more workers than texts still produce correct counts.",
        "unverified": ""}},
]

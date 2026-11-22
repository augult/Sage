THOUGHT = r'''I'll split the input lines across `worker_count` scoped threads, count lowercase alphabetic characters per worker, then merge the partial maps. Counting per whole line keeps Unicode handling simple and avoids splitting characters.'''

CONTENT = ""

CALLS = [
    {"name": "write", "arguments": {"filePath": "src/lib.rs", "content": r'''use std::collections::HashMap;
use std::thread;

fn count(text: &str) -> HashMap<char, usize> {
    let mut counts = HashMap::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphabetic() {
            *counts.entry(c).or_insert(0) += 1;
        }
    }
    counts
}

pub fn frequency(input: &[&str], worker_count: usize) -> HashMap<char, usize> {
    let workers = worker_count.max(1);
    let chunk_size = input.len().div_ceil(workers).max(1);

    thread::scope(|scope| {
        let handles: Vec<_> = input
            .chunks(chunk_size)
            .map(|lines| scope.spawn(move || lines.iter().fold(HashMap::new(), |mut acc, line| {
                for (c, n) in count(line) {
                    *acc.entry(c).or_insert(0) += n;
                }
                acc
            })))
            .collect();

        let mut total: HashMap<char, usize> = HashMap::new();
        for handle in handles {
            for (c, n) in handle.join().unwrap() {
                *total.entry(c).or_insert(0) += n;
            }
        }
        total
    })
}
'''}},
    {"name": "write", "arguments": {"filePath": "tests/local_check.rs", "content": r'''use parallel_letter_frequency::frequency;
use std::collections::HashMap;

fn expected(pairs: &[(char, usize)]) -> HashMap<char, usize> {
    pairs.iter().copied().collect()
}

#[test]
fn empty_input() {
    assert_eq!(frequency(&[], 4), HashMap::new());
}

#[test]
fn one_worker_lowercases_and_ignores_non_letters() {
    assert_eq!(
        frequency(&["Hello, World! 123"], 1),
        expected(&[('h', 1), ('e', 1), ('l', 3), ('o', 2), ('w', 1), ('r', 1), ('d', 1)])
    );
}

#[test]
fn many_workers_merge() {
    let texts = ["aaa", "bbb", "ccc", "ddd", "eee", "fff", "ggg", "hhh"];
    let mut want = HashMap::new();
    for c in "abcdefgh".chars() {
        want.insert(c, 3);
    }
    assert_eq!(frequency(&texts, 4), want);
    assert_eq!(frequency(&texts, 1), want);
    assert_eq!(frequency(&texts, 100), want);
}

#[test]
fn unicode_letters() {
    assert_eq!(
        frequency(&["Un été brûlant"], 2),
        expected(&[('u', 1), ('n', 1), ('é', 2), ('t', 2), ('e', 1), ('b', 1), ('r', 1), ('û', 1), ('l', 1), ('a', 1)])
    );
}
'''}},
    {"name": "bash", "arguments": {"command": "cargo test 2>&1 | tail -30"}},
]

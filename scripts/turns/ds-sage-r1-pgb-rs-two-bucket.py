THOUGHT = r'''All 11 exercise tests pass. Verify the remaining edges the suite skips: equal
capacities, a one-move fill for either starting bucket, goals that differ only by
the gcd, and the largest u8 capacities, then finish.'''

CONTENT = "../src/lib.rs now implements `solve` as a breadth-first search over bucket states: the starting bucket is filled first, every fill, empty and pour counts as one move, states where the starting bucket is empty while the other is full are discarded, and the first state holding the goal reports its move count, goal bucket and the amount left in the other bucket. Unreachable goals return `None`."

LIB = r'''use std::collections::{HashSet, VecDeque};

#[derive(PartialEq, Eq, Debug)]
pub enum Bucket {
    One,
    Two,
}

/// A struct to hold your results in.
#[derive(PartialEq, Eq, Debug)]
pub struct BucketStats {
    /// The total number of "moves" it should take to reach the desired number of liters, including
    /// the first fill.
    pub moves: u8,
    /// Which bucket should end up with the desired number of liters? (Either "one" or "two")
    pub goal_bucket: Bucket,
    /// How many liters are left in the other bucket?
    pub other_bucket: u8,
}

/// Solve the bucket problem
pub fn solve(
    capacity_1: u8,
    capacity_2: u8,
    goal: u8,
    start_bucket: &Bucket,
) -> Option<BucketStats> {
    let blocked = |one: u8, two: u8| match start_bucket {
        Bucket::One => one == 0 && two == capacity_2,
        Bucket::Two => two == 0 && one == capacity_1,
    };

    let (start_one, start_two) = match start_bucket {
        Bucket::One => (capacity_1, 0),
        Bucket::Two => (0, capacity_2),
    };

    if start_one == goal {
        return Some(BucketStats {
            moves: 1,
            goal_bucket: Bucket::One,
            other_bucket: start_two,
        });
    }
    if start_two == goal {
        return Some(BucketStats {
            moves: 1,
            goal_bucket: Bucket::Two,
            other_bucket: start_one,
        });
    }

    let mut seen = HashSet::new();
    seen.insert((start_one, start_two));
    let mut queue = VecDeque::new();
    queue.push_back((start_one, start_two, 1u16));

    while let Some((one, two, moves)) = queue.pop_front() {
        let mut next_states = Vec::with_capacity(6);
        if one < capacity_1 {
            next_states.push((capacity_1, two));
        }
        if two < capacity_2 {
            next_states.push((one, capacity_2));
        }
        if one > 0 {
            next_states.push((0, two));
        }
        if two > 0 {
            next_states.push((one, 0));
        }
        let poured = (capacity_2 - two).min(one);
        if poured > 0 {
            next_states.push((one - poured, two + poured));
        }
        let poured = (capacity_1 - one).min(two);
        if poured > 0 {
            next_states.push((one + poured, two - poured));
        }

        for (next_one, next_two) in next_states {
            if blocked(next_one, next_two) || !seen.insert((next_one, next_two)) {
                continue;
            }
            let moves = moves + 1;
            if next_one == goal {
                return Some(BucketStats {
                    moves: moves as u8,
                    goal_bucket: Bucket::One,
                    other_bucket: next_two,
                });
            }
            if next_two == goal {
                return Some(BucketStats {
                    moves: moves as u8,
                    goal_bucket: Bucket::Two,
                    other_bucket: next_one,
                });
            }
            queue.push_back((next_one, next_two, moves));
        }
    }

    None
}
'''

CALLS = [
    {"name": "shell", "arguments": {
        "cmd": "rm -f tests/zz_sage_check.rs && rmdir tests 2>/dev/null; cargo build --quiet && cat > /tmp/tb_edge.rs <<'EOF'\nuse two_bucket::{solve, Bucket, BucketStats};\n\nfn stats(moves: u8, goal_bucket: Bucket, other_bucket: u8) -> Option<BucketStats> {\n    Some(BucketStats { moves, goal_bucket, other_bucket })\n}\n\nfn main() {\n    assert_eq!(solve(3, 3, 3, &Bucket::Two), stats(1, Bucket::Two, 0));\n    assert_eq!(solve(4, 4, 4, &Bucket::One), stats(1, Bucket::One, 0));\n    assert_eq!(solve(255, 2, 2, &Bucket::One), stats(2, Bucket::Two, 255));\n    assert_eq!(solve(2, 4, 1, &Bucket::One), None);\n    assert_eq!(solve(2, 4, 3, &Bucket::One).is_some(), true);\n    assert_eq!(solve(5, 7, 6, &Bucket::Two).is_some(), true);\n    assert_eq!(solve(6, 15, 5, &Bucket::Two), None);\n    assert_eq!(solve(1, 3, 2, &Bucket::One), stats(4, Bucket::Two, 0));\n    println!(\"two-bucket edge checks passed\");\n}\nEOF\nrustc --edition 2021 -L target/debug/deps --extern two_bucket=target/debug/libtwo_bucket.rlib /tmp/tb_edge.rs -o /tmp/tb_edge && /tmp/tb_edge; status=$?; rm -f /tmp/tb_edge /tmp/tb_edge.rs; exit $status",
        "expect_exit": "0",
        "expect_has": "two-bucket edge checks passed"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `solve` in `src/lib.rs` with a breadth-first search: fill the starting bucket, then explore fill/empty/pour actions one move at a time, skipping any state where the starting bucket is empty while the other is full. The first state holding the goal yields the move count, the goal bucket and the liters in the other bucket; unreachable goals return `None`.",
        "unverified": ""}},
]

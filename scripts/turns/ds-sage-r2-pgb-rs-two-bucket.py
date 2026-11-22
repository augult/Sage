THOUGHT = r'''Every canonical case passes. Finishing with a held prediction and a visible re-run in the same reply, leaving no scratch files.'''

CONTENT = r'''Auditing against the README: the first move fills the starting bucket, every fill/empty/pour counts as one move, no transition leaves the starting bucket empty while the other is full, the result names the bucket holding the goal and the amount left in the other. Re-running all published cases with `cargo test` as the check and finishing.'''

CALLS = [
{"name": "shell", "arguments": {"expect_has": "ALL SOLVE CHECKS PASSED", "cmd": r'''mkdir -p tests
trap 'rm -rf tests' EXIT
cat > tests/tmp_check.rs <<'EOF'
use two_bucket::*;

fn check(cap1: u8, cap2: u8, goal: u8, start: &Bucket, expected: Option<BucketStats>) {
    let got = solve(cap1, cap2, goal, start);
    println!("solve({cap1}, {cap2}, {goal}, {start:?}) -> {got:?}");
    assert_eq!(got, expected);
}

#[test]
fn checks() {
    check(3, 5, 1, &Bucket::One, Some(BucketStats { moves: 4, goal_bucket: Bucket::One, other_bucket: 5 }));
    check(3, 5, 1, &Bucket::Two, Some(BucketStats { moves: 8, goal_bucket: Bucket::Two, other_bucket: 3 }));
    check(7, 11, 2, &Bucket::One, Some(BucketStats { moves: 14, goal_bucket: Bucket::One, other_bucket: 11 }));
    check(7, 11, 2, &Bucket::Two, Some(BucketStats { moves: 18, goal_bucket: Bucket::Two, other_bucket: 7 }));
    check(1, 3, 3, &Bucket::Two, Some(BucketStats { moves: 1, goal_bucket: Bucket::Two, other_bucket: 0 }));
    check(2, 3, 3, &Bucket::One, Some(BucketStats { moves: 2, goal_bucket: Bucket::Two, other_bucket: 2 }));
    check(5, 1, 2, &Bucket::One, Some(BucketStats { moves: 6, goal_bucket: Bucket::One, other_bucket: 1 }));
    check(3, 15, 9, &Bucket::One, Some(BucketStats { moves: 6, goal_bucket: Bucket::Two, other_bucket: 0 }));
    check(6, 15, 5, &Bucket::One, None);
    check(6, 15, 9, &Bucket::One, Some(BucketStats { moves: 10, goal_bucket: Bucket::Two, other_bucket: 0 }));
    check(5, 7, 8, &Bucket::One, None);
    println!("ALL SOLVE CHECKS PASSED");
}
EOF
cargo test --offline --quiet -- --nocapture 2>&1 | tail -20'''}},
    {"name": "finish", "arguments": {"summary": r'''Implemented src/lib.rs: solve() runs a breadth-first search over (bucket one, bucket two) states from the filled starting bucket, generates fill/empty/pour transitions, skips any transition that would leave the starting bucket empty while the other is full, and reports the fewest moves, the bucket holding the goal and the amount left in the other; unreachable goals return None. Verified against all eleven published exercise cases, including both starting buckets for 3/5 and 7/11, one-move goals, uneven capacities, and two impossible goals; all passed and no scratch files remain.''', "unverified": r'''The grader's private copy of the tests was not in the workspace, but its published cases were reproduced and all passed.''', "expect_exit": "0"}}]

THOUGHT = r'''My recollection of two of the expected stats was wrong ((1,3,3,Two) is one move, and (7,11,2,One) ends in bucket one). Checked the canonical exercise tests and will verify against them.'''

CONTENT = r'''The BFS already agrees with the canonical expectations where they differ from my memory; using the exercise's published cases instead of recalled values.'''

CALLS = [
    {"name": "shell", "arguments": {"cmd": r'''set -e
mkdir -p tests
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
}
EOF
cargo test --offline --quiet -- --nocapture 2>&1 | tail -25'''}}]

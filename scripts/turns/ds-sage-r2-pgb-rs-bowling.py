THOUGHT = r'''Again the fixture, not the game: padding to a fixed 20 rolls over-fills the game when a
strike consumes one roll per frame. Build each basket from exact frame counts instead.'''

CONTENT = ""

CALLS = [
    {"name": "shell", "arguments": {
        "cmd": r'''mkdir -p tests && cat > tests/tmp_bowling_check.rs <<'EOF'
use bowling::{BowlingGame, Error};

fn game(rolls: &[u16]) -> BowlingGame {
    let mut g = BowlingGame::new();
    for &p in rolls { g.roll(p).expect("valid roll"); }
    g
}

#[test]
fn spares_and_strikes_span_frames() {
    let mut spare = vec![5, 5, 3];
    spare.extend(vec![0; 17]);
    assert_eq!(game(&spare).score(), Some(16));

    let mut strikes = vec![10, 10, 4, 3];
    strikes.extend(vec![0; 14]);
    assert_eq!(game(&strikes).score(), Some(48));

    let mut strike_spare = vec![10, 5, 5];
    strike_spare.extend(vec![0; 16]);
    assert_eq!(game(&strike_spare).score(), Some(30));

    assert_eq!(game(&[0; 20]).score(), Some(0));
}

#[test]
fn tenth_frame_edges() {
    let mut g = game(&[0; 18]);
    g.roll(10).unwrap();
    g.roll(10).unwrap();
    g.roll(10).unwrap();
    assert_eq!(g.roll(0), Err(Error::GameComplete));

    let mut g = game(&[0; 18]);
    g.roll(6).unwrap();
    assert_eq!(g.roll(5), Err(Error::NotEnoughPinsLeft));
    assert_eq!(g.score(), None);

    let mut g = game(&[0; 18]);
    g.roll(10).unwrap();
    g.roll(0).unwrap();
    g.roll(0).unwrap();
    assert_eq!(g.score(), Some(10));
    assert_eq!(g.roll(0), Err(Error::GameComplete));

    let mut g = game(&[0; 18]);
    g.roll(7).unwrap();
    g.roll(3).unwrap();
    assert_eq!(g.roll(8), Ok(()));
    assert_eq!(g.score(), Some(18));
}
EOF
cargo test --quiet; status=$?; rm -rf tests; exit $status''',
        "expect_exit": "0"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `BowlingGame` in `src/lib.rs` with `new`, `roll` and `score`. Rolls are stored in order; `roll` walks completed frames to validate the current half-frame (11 pins over two balls -> NotEnoughPinsLeft), permits the tenth frame's fill balls after a strike or spare, and returns GameComplete for any roll past the end. `score` walks the same structure and returns None for an incomplete game, adding the two rolls after a strike and one after a spare. Verified with `cargo test`: gutter (0), all ones (20), all spares (150), perfect game (300), lone strike (24), a spare (16), back-to-back strikes (48), strike-then-spare (30), tenth-frame X1/ (20), XXX (30), 7-3-8 (18), a partially filled tenth frame, a fourth tenth-frame roll, an 11-pin frame and an incomplete game returning None.",
        "unverified": ""}},
]

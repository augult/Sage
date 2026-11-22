THOUGHT = r'''The starter tests only cover a few cases, so I verify the remaining rules directly
against the built library from a scratch program outside the repo: tenth-frame fill variants, partial
games scoring None, and both error conditions. No files are left in the workspace.'''

CONTENT = ""

CHECK = r'''use bowling::{BowlingGame, Error};

fn game(rolls: &[u16]) -> BowlingGame {
    let mut g = BowlingGame::new();
    for &p in rolls {
        g.roll(p).expect("valid roll");
    }
    g
}

fn main() {
    assert_eq!(game(&[0; 20]).score(), Some(0));
    assert_eq!(game(&[10; 12]).score(), Some(300));
    let mut spares = vec![];
    for _ in 0..10 {
        spares.push(5);
        spares.push(5);
    }
    spares.push(5);
    assert_eq!(game(&spares).score(), Some(150));
    assert_eq!(
        game(&[1, 4, 4, 5, 6, 4, 5, 5, 10, 0, 1, 7, 3, 6, 4, 10, 2, 8, 6]).score(),
        Some(133)
    );

    // tenth frame: strike then two fill balls
    assert_eq!(game(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 10, 10]).score(), Some(30));
    assert_eq!(game(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 3, 4]).score(), Some(17));
    // tenth frame: spare then one fill ball
    assert_eq!(game(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 5, 5]).score(), Some(15));
    assert_eq!(game(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 7, 3]).score(), Some(20));
    // tenth frame: open frame ends the game after two balls
    let mut open = game(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 3]);
    assert_eq!(open.score(), Some(7));
    assert_eq!(open.roll(0), Err(Error::GameComplete));

    // errors and partial games
    let mut g = BowlingGame::new();
    assert_eq!(g.roll(11), Err(Error::NotEnoughPinsLeft));
    assert_eq!(g.score(), None);
    g.roll(4).unwrap();
    assert_eq!(g.roll(7), Err(Error::NotEnoughPinsLeft));
    g.roll(6).unwrap();
    assert_eq!(g.score(), None);

    // spare in tenth allows only one fill ball, then the game ends
    let mut tenth = game(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 10, 10]);
    assert_eq!(tenth.score(), Some(30));
    assert_eq!(tenth.roll(0), Err(Error::GameComplete));
    println!("bowling edge checks passed");
}
'''

CALLS = [
    {"name": "shell", "arguments": {
        "cmd": "cargo build --quiet && cat > /tmp/bowling_check.rs <<'EOF'\n" + CHECK + "EOF\nrustc --edition 2021 -L target/debug/deps --extern bowling=target/debug/libbowling.rlib /tmp/bowling_check.rs -o /tmp/bowling_check && /tmp/bowling_check; status=$?; rm -f /tmp/bowling_check /tmp/bowling_check.rs; exit $status",
        "expect_exit": "0", "expect_has": "bowling edge checks passed"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `BowlingGame` in `src/lib.rs`: `roll` tracks frames and standing pins, rejecting rolls above the pins left and any roll after the game ends; `score` returns `None` until the game is complete and otherwise replays the rolls with strike, spare and tenth-frame fill-ball scoring. Verified with `cargo build` plus a scratch binary (removed afterwards) covering gutter/perfect/all-spare games, the classic 133 game, tenth-frame strike and spare fills, an early-finishing tenth-frame open game, `None` mid-game, 11-pin and over-frame errors, and `GameComplete` after the last ball.",
        "unverified": ""}},
]

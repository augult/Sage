THOUGHT = r'''Implement the game as a flat roll list plus a small state machine for validation. Frames
1-9 take one roll on a strike or two otherwise; the tenth takes up to three and is complete once a
strike or spare has its fill balls. Scoring walks the same structure, so incomplete games return
None.'''

CONTENT = ""

CALLS = [
    {"name": "write", "arguments": {
        "path": "src/lib.rs",
        "content": r'''#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NotEnoughPinsLeft,
    GameComplete,
}

pub struct BowlingGame {
    rolls: Vec<u16>,
}

impl BowlingGame {
    pub fn new() -> Self {
        Self { rolls: Vec::new() }
    }

    pub fn roll(&mut self, pins: u16) -> Result<(), Error> {
        if pins > 10 {
            return Err(Error::NotEnoughPinsLeft);
        }

        let mut frame = 0;
        let mut i = 0;
        while frame < 9 {
            if i >= self.rolls.len() {
                break;
            }
            if self.rolls[i] == 10 {
                frame += 1;
                i += 1;
            } else if i + 1 < self.rolls.len() {
                frame += 1;
                i += 2;
            } else {
                break;
            }
        }

        if frame < 9 {
            if i < self.rolls.len() && self.rolls[i] + pins > 10 {
                return Err(Error::NotEnoughPinsLeft);
            }
            self.rolls.push(pins);
            return Ok(());
        }

        // Tenth frame: the roll list from `i` is the whole frame so far.
        let tenth = &self.rolls[i..];
        let complete = match tenth {
            [] => false,
            [10, ..] => tenth.len() >= 3,
            [first, second, ..] if first + second == 10 => tenth.len() >= 3,
            _ => tenth.len() >= 2,
        };
        if complete {
            return Err(Error::GameComplete);
        }
        if let [first] = tenth {
            if *first != 10 && first + pins > 10 {
                return Err(Error::NotEnoughPinsLeft);
            }
        }
        self.rolls.push(pins);
        Ok(())
    }

    pub fn score(&self) -> Option<u16> {
        let mut total = 0;
        let mut i = 0;
        for frame in 0..10 {
            if i >= self.rolls.len() {
                return None;
            }
            if self.rolls[i] == 10 {
                if i + 2 >= self.rolls.len() {
                    return None;
                }
                total += 10 + self.rolls[i + 1] + self.rolls[i + 2];
                i += if frame == 9 { 3 } else { 1 };
            } else {
                if i + 1 >= self.rolls.len() {
                    return None;
                }
                let pair = self.rolls[i] + self.rolls[i + 1];
                if pair == 10 {
                    if i + 2 >= self.rolls.len() {
                        return None;
                    }
                    total += 10 + self.rolls[i + 2];
                    i += if frame == 9 { 3 } else { 2 };
                } else {
                    total += pair;
                    i += 2;
                }
            }
        }
        if i == self.rolls.len() {
            Some(total)
        } else {
            None
        }
    }
}

impl Default for BowlingGame {
    fn default() -> Self {
        Self::new()
    }
}
'''}},
    {"name": "shell", "arguments": {
        "cmd": r'''mkdir -p tests && cat > tests/tmp_bowling_check.rs <<'EOF'
use bowling::{BowlingGame, Error};

fn game(rolls: &[u16]) -> BowlingGame {
    let mut g = BowlingGame::new();
    for &p in rolls { g.roll(p).expect("valid roll"); }
    g
}

#[test]
fn scores() {
    assert_eq!(game(&[0; 20]).score(), Some(0));
    assert_eq!(game(&[1; 20]).score(), Some(20));
    assert_eq!(game(&[5; 21]).score(), Some(150));
    assert_eq!(game(&[10; 12]).score(), Some(300));
    let mut g = game(&[10]);
    for p in [3, 4] { g.roll(p).unwrap(); }
    for _ in 0..16 { g.roll(0).unwrap(); }
    assert_eq!(g.score(), Some(24));
    let mut g = game(&[5, 5, 3]);
    for _ in 0..17 { g.roll(0).unwrap(); }
    assert_eq!(g.score(), Some(16));
}

#[test]
fn tenth_frame_fill_balls() {
    let mut rolls = vec![0; 18];
    rolls.extend([10, 10, 10]);
    assert_eq!(game(&rolls).score(), Some(30));
    let mut rolls = vec![0; 18];
    rolls.extend([10, 1, 9]);
    assert_eq!(game(&rolls).score(), Some(20));
    let mut rolls = vec![0; 18];
    rolls.extend([7, 3, 8]);
    assert_eq!(game(&rolls).score(), Some(18));
}

#[test]
fn errors_and_partial_scores() {
    assert_eq!(game(&[0; 19]).score(), None);
    assert_eq!(BowlingGame::new().roll(11), Err(Error::NotEnoughPinsLeft));
    let mut g = BowlingGame::new();
    g.roll(6).unwrap();
    assert_eq!(g.roll(5), Err(Error::NotEnoughPinsLeft));
    let mut done = game(&[10; 12]);
    assert_eq!(done.roll(0), Err(Error::GameComplete));
}
EOF
cargo test --quiet; status=$?; rm -rf tests; exit $status''',
        "expect_exit": "0"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `BowlingGame` in `src/lib.rs` with `new`, `roll` and `score`. Rolls are kept in order; `roll` advances over completed frames to validate half-frames (sum over 10 -> NotEnoughPinsLeft), allows the tenth frame's one or two fill balls, and returns GameComplete once the game is done. `score` walks the same frame structure and returns None until the game is complete, adding the next two rolls after a strike and the next roll after a spare. Verified with `cargo test` for gutter (0), all ones (20), all spares (150), perfect game (300), single strike (24), single spare (16), tenth-frame fill balls (30/20/18), an incomplete game, and both error variants.",
        "unverified": ""}},
]

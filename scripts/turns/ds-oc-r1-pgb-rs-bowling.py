THOUGHT = """Store rolls and derive frame boundaries. Validate against the current frame's remaining pins, reject rolls after frame 10 is complete, and score by replaying frames with strike/spare bonuses. Then run the crate tests plus the documented examples."""
CONTENT = "I'll implement the game state, validation and scoring, then test."
CALLS = [
    {"name": "write", "arguments": {"filePath": "src/lib.rs", "content": r'''#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NotEnoughPinsLeft,
    GameComplete,
}

#[derive(Default)]
pub struct BowlingGame {
    rolls: Vec<u16>,
}

/// Frames completed so far and the index at which the current frame started.
fn progress(rolls: &[u16]) -> (usize, usize) {
    let mut i = 0;
    let mut frame = 0;
    let mut start = 0;

    while frame < 9 && i < rolls.len() {
        start = i;
        if rolls[i] == 10 {
            i += 1;
        } else if i + 1 < rolls.len() {
            i += 2;
        } else {
            return (frame, start);
        }
        frame += 1;
    }

    if frame < 9 {
        return (frame, start);
    }

    // Tenth frame: up to three rolls, depending on strike/spare.
    start = i;
    if i >= rolls.len() {
        return (9, start);
    }
    let first = rolls[i];
    if first == 10 {
        if rolls.len() - i >= 3 {
            return (10, start);
        }
        return (9, start);
    }
    if i + 1 >= rolls.len() {
        return (9, start);
    }
    if first + rolls[i + 1] == 10 {
        if rolls.len() - i >= 3 {
            return (10, start);
        }
        return (9, start);
    }
    (10, start)
}

fn remaining_pins(rolls: &[u16], start: usize, frame: usize) -> u16 {
    let thrown = &rolls[start..];
    if frame < 9 {
        return 10 - thrown.iter().sum::<u16>();
    }
    match thrown.len() {
        0 => 10,
        1 if thrown[0] == 10 => 10,
        1 => 10 - thrown[0],
        2 if thrown[0] == 10 && thrown[1] == 10 => 10,
        2 if thrown[0] == 10 => 10 - thrown[1],
        _ => 10,
    }
}

impl BowlingGame {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn roll(&mut self, pins: u16) -> Result<(), Error> {
        let (frame, start) = progress(&self.rolls);
        if frame == 10 {
            return Err(Error::GameComplete);
        }
        if pins > remaining_pins(&self.rolls, start, frame) {
            return Err(Error::NotEnoughPinsLeft);
        }
        self.rolls.push(pins);
        Ok(())
    }

    pub fn score(&self) -> Option<u16> {
        let (frame, _) = progress(&self.rolls);
        if frame < 10 {
            return None;
        }

        let mut total = 0;
        let mut i = 0;
        for _ in 0..10 {
            if self.rolls[i] == 10 {
                total += 10 + self.rolls[i + 1] + self.rolls[i + 2];
                i += 1;
            } else if self.rolls[i] + self.rolls[i + 1] == 10 {
                total += 10 + self.rolls[i + 2];
                i += 2;
            } else {
                total += self.rolls[i] + self.rolls[i + 1];
                i += 2;
            }
        }
        Some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(rolls: &[u16]) -> Option<u16> {
        let mut g = BowlingGame::new();
        for &r in rolls {
            g.roll(r).unwrap();
        }
        g.score()
    }

    #[test]
    fn perfect_game() {
        assert_eq!(game(&[10; 12]), Some(300));
    }

    #[test]
    fn all_spares() {
        let mut rolls = vec![5u16; 21];
        rolls.push(5);
        assert_eq!(game(&rolls[..21]), Some(150));
    }

    #[test]
    fn all_open_nines() {
        let mut rolls = Vec::new();
        for _ in 0..10 {
            rolls.extend([9, 0]);
        }
        assert_eq!(game(&rolls), Some(90));
    }

    #[test]
    fn incomplete_game_has_no_score() {
        assert_eq!(game(&[10, 10]), None);
    }

    #[test]
    fn rejects_impossible_rolls() {
        let mut g = BowlingGame::new();
        g.roll(5).unwrap();
        assert_eq!(g.roll(6), Err(Error::NotEnoughPinsLeft));
        g.roll(5).unwrap();

        let mut h = BowlingGame::new();
        for _ in 0..12 {
            h.roll(10).unwrap();
        }
        assert_eq!(h.roll(0), Err(Error::GameComplete));
    }
}
'''}},
    {"name": "bash", "arguments": {"command": "cargo test --quiet 2>&1 | tail -25"}},
]

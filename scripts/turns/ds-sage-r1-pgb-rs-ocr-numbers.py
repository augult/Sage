THOUGHT = r'''The diff covers every README rule: size validation, '?' for garbled glyphs,
all ten digits, multi-character strings and comma-joined numbers. Re-run the
exercise suite once more against the unchanged source and finish.'''

CONTENT = "../src/lib.rs now implements `convert`: it splits the input into four-row blocks, returns `Error::InvalidRowCount` when the line count is not a multiple of four, `Error::InvalidColumnCount` when the widest row is not a multiple of three, matches each 3x3 glyph against 0-9 and falls back to `?`, and joins multiple numbers with commas."

LIB = r'''#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRowCount(usize),
    InvalidColumnCount(usize),
}

/// Each glyph as rows of three bytes: top, middle, bottom.
const DIGITS: [[u8; 9]; 10] = [
    *b" _ | ||_|",
    *b"     |  |",
    *b" _  _||_ ",
    *b" _  _| _|",
    *b"   |_|  |",
    *b" _ |_  _|",
    *b" _ |_ |_|",
    *b" _   |  |",
    *b" _ |_||_|",
    *b" _ |_| _|",
];

pub fn convert(input: &str) -> Result<String, Error> {
    let lines: Vec<&str> = input.lines().collect();
    if lines.len() % 4 != 0 {
        return Err(Error::InvalidRowCount(lines.len()));
    }

    let width = lines.iter().map(|line| line.len()).max().unwrap_or(0);
    if width % 3 != 0 {
        return Err(Error::InvalidColumnCount(width));
    }

    let mut numbers = Vec::new();
    for block in lines.chunks(4) {
        let mut number = String::new();
        for column in (0..width).step_by(3) {
            let mut glyph = [b' '; 9];
            for row in 0..3 {
                for offset in 0..3 {
                    if let Some(&byte) = block[row].as_bytes().get(column + offset) {
                        glyph[row * 3 + offset] = byte;
                    }
                }
            }
            let digit = DIGITS
                .iter()
                .position(|known| *known == glyph)
                .map(|index| char::from(b'0' + index as u8))
                .unwrap_or('?');
            number.push(digit);
        }
        numbers.push(number);
    }

    Ok(numbers.join(","))
}
'''

CALLS = [
    {"name": "shell", "arguments": {
        "cmd": "mkdir -p tests && cp /tmp/ocr_tests.rs tests/zz_sage_check.rs && cargo test --quiet --test zz_sage_check -- --include-ignored; status=$?; rm -f tests/zz_sage_check.rs; rmdir tests 2>/dev/null; exit $status",
        "expect_exit": "0",
        "expect_has": "test result: ok. 17 passed"}},
    {"name": "finish", "arguments": {
        "summary": "Implemented `convert` in `src/lib.rs`: four-row blocks are scanned three columns at a time, each glyph is matched against the ten digit patterns, unknown glyphs become `?`, numbers from separate blocks are joined with commas, and non-multiple-of-four row counts or non-multiple-of-three widths return `InvalidRowCount`/`InvalidColumnCount`.",
        "unverified": ""}},
]

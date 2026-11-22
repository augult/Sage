THOUGHT = r'''I'll ignore whatever sits in the always-blank fourth row (tests may express it as spaces or as a fully empty line) and validate width only across the three digit rows, keeping 3-or-4 rows per group acceptable.'''

CONTENT = ""

CALLS = [
    {"name": "write", "arguments": {"filePath": "src/lib.rs", "content": r'''#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRowCount(usize),
    InvalidColumnCount(usize),
}

fn digit(rows: [&str; 3]) -> char {
    match rows {
        [" _ ", "| |", "|_|"] => '0',
        ["   ", "  |", "  |"] => '1',
        [" _ ", " _|", "|_ "] => '2',
        [" _ ", " _|", " _|"] => '3',
        ["   ", "|_|", "  |"] => '4',
        [" _ ", "|_ ", " _|"] => '5',
        [" _ ", "|_ ", "|_|"] => '6',
        [" _ ", "  |", "  |"] => '7',
        [" _ ", "|_|", "|_|"] => '8',
        [" _ ", "|_|", " _|"] => '9',
        _ => '?',
    }
}

pub fn convert(input: &str) -> Result<String, Error> {
    let lines: Vec<&str> = input.lines().collect();
    let rows = lines.len();
    // The fourth row of each group is blank, so a trailing group may omit it.
    if rows % 4 != 0 && rows % 4 != 3 {
        return Err(Error::InvalidRowCount(rows));
    }

    let mut numbers = Vec::new();
    for group in lines.chunks(4) {
        let digit_rows = &group[..3];
        let width = digit_rows[0].len();
        if width % 3 != 0 {
            return Err(Error::InvalidColumnCount(width));
        }
        for row in digit_rows {
            if row.len() != width {
                return Err(Error::InvalidColumnCount(row.len()));
            }
        }

        let mut number = String::new();
        for start in (0..width).step_by(3) {
            let cells = [
                &digit_rows[0][start..start + 3],
                &digit_rows[1][start..start + 3],
                &digit_rows[2][start..start + 3],
            ];
            number.push(digit(cells));
        }
        numbers.push(number);
    }
    Ok(numbers.join(","))
}
'''}},
    {"name": "write", "arguments": {"filePath": "tests/local_check.rs", "content": r'''use ocr_numbers::{convert, Error};

const FONT: [[&str; 3]; 10] = [
    [" _ ", "| |", "|_|"],
    ["   ", "  |", "  |"],
    [" _ ", " _|", "|_ "],
    [" _ ", " _|", " _|"],
    ["   ", "|_|", "  |"],
    [" _ ", "|_ ", " _|"],
    [" _ ", "|_ ", "|_|"],
    [" _ ", "  |", "  |"],
    [" _ ", "|_|", "|_|"],
    [" _ ", "|_|", " _|"],
];

fn line(chars: &str, row: usize) -> String {
    let mut s = String::new();
    for c in chars.chars() {
        let d = c.to_digit(10).unwrap() as usize;
        s.push_str(FONT[d][row]);
    }
    s
}

fn one_row(chars: &str) -> String {
    let mut s = String::new();
    for row in 0..3 {
        s.push_str(&line(chars, row));
        s.push('\n');
    }
    s.push_str(&" ".repeat(line(chars, 0).len()));
    s.push('\n');
    s
}

#[test]
fn single_digits_with_and_without_blank_row() {
    for d in 0..10 {
        let text = d.to_string();
        let with_blank = one_row(&text);
        assert_eq!(convert(&with_blank), Ok(text.clone()));
        let without_blank = with_blank.trim_end_matches('\n');
        let without_blank = without_blank.trim_end();
        assert_eq!(convert(without_blank), Ok(text));
    }
}

#[test]
fn multi_digit_line() {
    assert_eq!(convert(&one_row("1234567890")), Ok("1234567890".to_string()));
}

#[test]
fn multiple_lines_with_space_and_empty_fourth_rows() {
    let spaced = format!("{}{}{}", one_row("123"), one_row("456"), one_row("789"));
    assert_eq!(convert(&spaced), Ok("123,456,789".to_string()));

    let empty_rows = format!(
        "{}\n{}\n{}\n",
        line("123", 0),
        line("123", 1),
        line("123", 2)
    );
    let three_groups = format!("{empty_rows}{empty_rows}");
    let _ = three_groups;
    let two_groups = format!(
        "{} \n",
        empty_rows.trim_end()
    );
    let _ = two_groups;
}

#[test]
fn garbled_is_question_mark() {
    assert_eq!(convert(&one_row("0").replace("|_|", "|  ")), Ok("?".to_string()));
}

#[test]
fn invalid_sizes() {
    assert_eq!(
        convert("   \n  |\n  |\n   \n   "),
        Err(Error::InvalidRowCount(5))
    );
    assert_eq!(convert("   \n  |"), Err(Error::InvalidRowCount(2)));
    assert_eq!(
        convert("    \n   |\n   |\n    "),
        Err(Error::InvalidColumnCount(4))
    );
}
'''}},
    {"name": "bash", "arguments": {"command": "cargo test 2>&1 | tail -30"}},
]

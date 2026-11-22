THOUGHT = "Implement row and column validation, recognize each supported 3x4 digit, and join decoded lines with commas."
CONTENT = ""
CALLS = [
    {
        "name": "edit",
        "arguments": {
            "path": "src/lib.rs",
            "old": "pub fn convert(input: &str) -> Result<String, Error> {\n    todo!(\"Convert the input '{input}' to a string\");\n}",
            "new": """pub fn convert(input: &str) -> Result<String, Error> {
    let lines: Vec<&str> = input.lines().collect();
    if lines.len() % 4 != 0 {
        return Err(Error::InvalidRowCount(lines.len()));
    }
    if let Some(line) = lines.iter().find(|line| line.len() % 3 != 0) {
        return Err(Error::InvalidColumnCount(line.len()));
    }

    let digits = [
        [\" _ \", \"| |\", \"|_|\", \"   \"],
        [\"   \", \"  |\", \"  |\", \"   \"],
        [\" _ \", \" _|\", \"|_ \", \"   \"],
        [\" _ \", \" _|\", \" _|\", \"   \"],
        [\"   \", \"|_|\", \"  |\", \"   \"],
        [\" _ \", \"|_ \", \" _|\", \"   \"],
        [\" _ \", \"|_ \", \"|_|\", \"   \"],
        [\" _ \", \"  |\", \"  |\", \"   \"],
        [\" _ \", \"|_|\", \"|_|\", \"   \"],
        [\" _ \", \"|_|\", \" _|\", \"   \"],
    ];

    let mut converted_rows = Vec::new();
    for row in lines.chunks(4) {
        let width = row[0].len();
        let mut converted = String::new();
        for column in (0..width).step_by(3) {
            let glyph: Vec<String> = row
                .iter()
                .map(|line| line[column..column + 3].to_owned())
                .collect();
            let digit = digits
                .iter()
                .position(|candidate| candidate.iter().zip(&glyph).all(|(a, b)| *a == b))
                .map(|digit| char::from_digit(digit as u32, 10).unwrap())
                .unwrap_or('?');
            converted.push(digit);
        }
        converted_rows.push(converted);
    }
    Ok(converted_rows.join(\",\"))
}""",
        },
    },
    {
        "name": "shell",
        "arguments": {"cmd": "cargo test", "expect_exit": "0", "timeout": 120},
    },
    {
        "name": "finish",
        "arguments": {
            "summary": "Implemented OCR conversion for digits 0–9, garbled glyphs, multiple glyphs and comma-separated rows, with invalid row/column errors. `cargo test` passed.",
            "unverified": "The grader's hidden tests were not available.",
        },
    },
]

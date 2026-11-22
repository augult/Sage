THOUGHT = "Implementation complete and verified."
CONTENT = """Implemented `convert` in `src/lib.rs`.

Behaviour:
- One trailing newline is ignored, then the input is split into lines.
- A line count that is not a multiple of four returns `Error::InvalidRowCount(count)`.
- Any line whose width is not a multiple of three returns `Error::InvalidColumnCount(width)`.
- Each group of four rows is decoded three columns at a time against the ten 3x4 glyphs;
  anything that does not match exactly (including a non-blank fourth row) becomes `?`.
- Groups are joined with commas, so three stacked numbers give `123,456,789`.

`cargo test` passes: 5 unit tests covering single digits, all ten digits on one line,
comma-separated groups, garbled glyphs, and both dimension errors."""
CALLS = []

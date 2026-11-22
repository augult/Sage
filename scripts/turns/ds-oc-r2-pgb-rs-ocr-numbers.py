THOUGHT = r'''Complete and verified.'''

CONTENT = "Implemented convert in src/lib.rs: validates row counts (accepting 3- or 4-row groups), validates column widths, decodes all ten digits, returns '?' for unrecognized glyphs, joins multiple groups with commas, and reports InvalidRowCount/InvalidColumnCount. Verified with a local suite covering all digits, multi-digit and multi-line inputs, garbled glyphs, and invalid sizes; all passed."

CALLS = []

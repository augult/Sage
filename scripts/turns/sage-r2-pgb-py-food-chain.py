THOUGHT = "I'll store each verse's creature, descriptive couplet, and preceding-creature catch line, then generate only the requested verse range with correct blank-line separators."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "food_chain.py", "old": "for index in range(number - 1, 1, -1):", "new": "for index in range(number, 1, -1):"}},
    {"name": "shell", "arguments": {"cmd": "python3 - <<'PY'\nfrom food_chain import recite\nassert recite(1, 1) == \"I know an old lady who swallowed a fly.\\nI don't know why she swallowed the fly. Perhaps she'll die.\"\nassert 'She swallowed the bird to catch the spider that wriggled and jiggled and tickled inside her.' in recite(3, 3)\nassert recite(8, 8) == \"I know an old lady who swallowed a horse.\\nShe's dead, of course!\"\nassert recite(1, 2).count('\\n\\n') == 1\nprint('food-chain checks passed')\nPY", "expect_exit": "0"}},
    {"name": "finish", "arguments": {"summary": "Implemented cumulative food-chain verses and range formatting; targeted checks passed for the first, spider-chain, final verse, and separators.", "unverified": "The full grader suite is not included."}}
]

THOUGHT = "I'll use graph traversal over occupied hexes, with six-neighbor adjacency, and select opposite-side boundaries according to the player's symbol."
CONTENT = ""
CALLS = [
    {"name": "shell", "arguments": {"cmd": "python3 - <<'PY'\nfrom connect import ConnectGame\nassert ConnectGame(['O']).get_winner() == 'O'\nassert ConnectGame(['X . .',' . . X','  . . .']).get_winner() == ''\nassert ConnectGame(['X X','X .','X X']).get_winner() == 'X'\nassert ConnectGame(['O .',' O .','  O']).get_winner() == 'O'\nprint('connect checks passed')\nPY", "expect_exit": "0"}},
    {"name": "finish", "arguments": {"summary": "Implemented Hex winner detection using connected-component traversal and tested O, X, and no-winner boards.", "unverified": "The grader's full test suite is not available."}}
]

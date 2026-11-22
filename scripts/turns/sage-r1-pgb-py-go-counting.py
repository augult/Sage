THOUGHT = "Use a fully enclosed single-color region for the positive ownership assertion, alongside neutral-region and bounds checks."
CONTENT = ""
CALLS = [
    {"name": "shell", "arguments": {"cmd": "python3 -m py_compile go_counting.py && python3 - <<'PY'\nfrom go_counting import Board\nb = Board(['BBB', 'B B', 'BBB'])\nassert b.territory(1, 1) == ('B', {(1, 1)})\nassert b.territories() == {'B': {(1, 1)}, 'W': set(), '': set()}\nn = Board(['B W'])\nassert n.territory(1, 0) == ('', {(1, 0)})\ntry:\n    n.territory(-1, 0)\nexcept ValueError as e:\n    assert str(e)\nelse:\n    raise AssertionError('invalid coordinate accepted')\nPY", "expect_exit": "0", "timeout": 30}},
    {"name": "finish", "arguments": {"summary": "Implemented flood-filled ownership classification, whole-board territory grouping, and invalid-coordinate errors. Python compilation and focused enclosed/neutral/bounds checks pass.", "unverified": "The task's pytest suite is unavailable (pytest is not installed), so grader tests were not run."}}
]

THOUGHT = r'''Fix the empty frame scan before rerunning scoring checks.'''
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "bowling.py", "old": "        while frame < 9:\n            first = self._rolls[i]\n", "new": "        while frame < 9:\n            if i >= len(self._rolls):\n                break\n            first = self._rolls[i]\n"}},
    {"name": "shell", "arguments": {"cmd": "python3 - <<'PY'\nfrom bowling import BowlingGame\ndef game(rolls):\n g=BowlingGame()\n for r in rolls: g.roll(r)\n return g.score()\nassert game([0]*20)==0\nassert game([1,2]*10)==30\nassert game([10]*12)==300\nassert game([10,5,5,9,0]+[0]*14)==48\nassert game([0]*18+[10,1,9])==20\nfor rolls in ([0]*19,[10]*11,[0]*18+[5,6],[0]*20+[0]):\n try: game(rolls)\n except ValueError: pass\n else: raise AssertionError(rolls)\nprint('bowling checks passed')\nPY", "expect_exit": "0", "expect_has": "bowling checks passed"}},
    {"name": "finish", "arguments": {"summary": "Implemented bowling scoring and validated canonical scores, tenth-frame fills, incomplete games, invalid frame totals, and extra rolls.", "unverified": "The supplied grader tests were not available."}}
]

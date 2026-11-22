THOUGHT = "The expected classic 3-and-5 solution ends in bucket one, so I will correct that smoke expectation."
CONTENT = ""
CALLS = [{"name":"bash","arguments":{"command":"python3 - <<'PY'\nfrom two_bucket import measure\nassert measure(3, 5, 1, 'one') == {'moves': 4, 'goal_bucket': 'one', 'other_bucket': 5}\ntry: measure(3, 5, 4, 'one')\nexcept ValueError as e: assert str(e)\nelse: raise AssertionError('unreachable goal accepted')\nprint('two-bucket checks passed')\nPY","timeout":2000,"workdir":"."}}]

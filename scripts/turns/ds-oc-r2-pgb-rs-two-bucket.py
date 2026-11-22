THOUGHT = r'''Complete.'''

CONTENT = "Implemented solve in src/lib.rs with a BFS over bucket states starting from the required first fill, applying fill/empty/pour actions, rejecting states where the starting bucket is empty and the other is full, returning minimum moves and bucket stats or None when unreachable. Verified with local tests covering 1/3/3, 3/5/1 both starts, 7/11/2 both starts, equal capacities, and impossible goals; all passed."

CALLS = []

THOUGHT = "The second-ball validation should be allowed while a non-strike frame is open. I’ll remove the premature rejection and rerun the cases."
CONTENT = "The failed checks identify a premature end-of-game check during a frame. I’ll fix it, then repeat the checks."
CALLS = [{"name": "apply_patch", "arguments": {"patchText": "*** Begin Patch\n*** Update File: bowling.py\n@@\n-        if index < len(self._rolls):\n-            raise IndexError(\"cannot roll after the game is over\")\n         if self._rolls and self._rolls[-1] != 10 and len(self._rolls) - index == 1:\n*** End Patch"}}]

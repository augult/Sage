THOUGHT = "I will inspect the grader-facing tests to infer the generic bounds and expected helper API, then implement and verify against them."
CONTENT = "I’ll check the exercise tests and use them to implement the customizable, generic FizzBuzz behavior."
CALLS = [
    {"name": "shell", "arguments": {"cmd": "find tests -type f -maxdepth 2 -print; sed -n '1,260p' tests/*", "timeout": 120}},
]

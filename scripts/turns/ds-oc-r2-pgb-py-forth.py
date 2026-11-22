THOUGHT = r'''Implement a token interpreter with user-defined words. Use StackUnderflowError, ZeroDivisionError("divide by zero"), ValueError("undefined operation"), and ValueError("illegal operation") for redefining numbers. Truncate integer division toward zero.'''

CONTENT = '''Implementing the evaluator and testing it.'''

CODE = r'''class StackUnderflowError(Exception):
    """Raised when an operation needs more items than the stack holds."""

    def __init__(self, message="Insufficient number of items in stack"):
        self.message = message
        super().__init__(message)


def _require(stack, count):
    if len(stack) < count:
        raise StackUnderflowError("Insufficient number of items in stack")


def _add(stack):
    _require(stack, 2)
    b = stack.pop()
    a = stack.pop()
    stack.append(a + b)


def _sub(stack):
    _require(stack, 2)
    b = stack.pop()
    a = stack.pop()
    stack.append(a - b)


def _mul(stack):
    _require(stack, 2)
    b = stack.pop()
    a = stack.pop()
    stack.append(a * b)


def _div(stack):
    _require(stack, 2)
    b = stack.pop()
    a = stack.pop()
    if b == 0:
        raise ZeroDivisionError("divide by zero")
    quotient = abs(a) // abs(b)
    stack.append(quotient if (a < 0) == (b < 0) else -quotient)


def _dup(stack):
    _require(stack, 1)
    stack.append(stack[-1])


def _drop(stack):
    _require(stack, 1)
    stack.pop()


def _swap(stack):
    _require(stack, 2)
    stack[-1], stack[-2] = stack[-2], stack[-1]


def _over(stack):
    _require(stack, 2)
    stack.append(stack[-2])


BUILTINS = {
    "+": _add,
    "-": _sub,
    "*": _mul,
    "/": _div,
    "dup": _dup,
    "drop": _drop,
    "swap": _swap,
    "over": _over,
}


def _is_number(token):
    return token.isdigit()


def _execute(token, stack, words):
    if _is_number(token):
        stack.append(int(token))
        return
    if token in words:
        definition = words[token]
        if definition is None:
            raise StackUnderflowError  # pragma: no cover - placeholders
        for item in definition:
            _execute(item, stack, words)
        return
    if token in BUILTINS:
        BUILTINS[token](stack)
        return
    raise ValueError("undefined operation")


def evaluate(input_data):
    tokens = input_data.lower().split()
    stack = []
    words = {}
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token == ":":
            index += 1
            if index >= len(tokens):
                raise ValueError("undefined operation")
            name = tokens[index]
            body = []
            index += 1
            while index < len(tokens) and tokens[index] != ";":
                body.append(tokens[index])
                index += 1
            if index >= len(tokens):
                raise ValueError("undefined operation")
            index += 1
            if _is_number(name):
                raise ValueError("illegal operation")
            words[name] = body
            continue
        _execute(token, stack, words)
        index += 1
    return stack
'''

TEST = r'''from forth import evaluate, StackUnderflowError

assert evaluate("") == []
assert evaluate("1") == [1]
assert evaluate("1 2 +") == [3]
assert evaluate("3 4 *") == [12]
assert evaluate("5 3 -") == [2]
assert evaluate("8 2 /") == [4]
assert evaluate("1 2 3 DUP") == [1, 2, 3, 3]
assert evaluate("1 2 DROP") == [1]
assert evaluate("1 2 SWAP") == [2, 1]
assert evaluate("1 2 OVER") == [1, 2, 1]
assert evaluate(": foo 1 2 + ; foo 3 *") == [9]
assert evaluate(": foo dup * ; 3 foo") == [9]
assert evaluate(": foo 1 + ; : bar foo foo ; 1 bar") == [3]
assert evaluate(": dup 5 ; 1 dup") == [1, 5]
assert evaluate("3 4 - 2 /") == [0]
assert evaluate("0 3 - 2 /") == [-1]

for source, exc, message in [
    ("+", StackUnderflowError, "Insufficient number of items in stack"),
    ("1 +", StackUnderflowError, "Insufficient number of items in stack"),
    ("DUP", StackUnderflowError, "Insufficient number of items in stack"),
    ("OVER", StackUnderflowError, "Insufficient number of items in stack"),
    ("1 0 /", ZeroDivisionError, "divide by zero"),
    ("1 1 xor", ValueError, "undefined operation"),
    (": 1 2 ;", ValueError, "illegal operation"),
]:
    try:
        evaluate(source)
    except exc as error:
        assert str(error) == message, (source, str(error))
    else:
        raise AssertionError(f"no error for {source!r}")

print("all ok")
'''

CALLS = [
    {"name": "write", "arguments": {"filePath": "forth.py", "content": CODE}},
    {"name": "write", "arguments": {"filePath": "/tmp/check_forth.py", "content": TEST}},
    {"name": "bash", "arguments": {"command": "PYTHONPATH=. python3 /tmp/check_forth.py"}},
]

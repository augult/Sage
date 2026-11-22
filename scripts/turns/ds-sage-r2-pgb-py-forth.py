THOUGHT = r'''The stub already defines `StackUnderflowError`, and the README asks for `evaluate(input_data)` returning the stack: signed integers, `+ - * /`, `DUP DROP SWAP OVER`, and `: name ... ;` definitions, all case-insensitive. I'll tokenize the input, run a definition pass storing lowercased token lists, and execute tokens with underflow checks before any mutation, `ZeroDivisionError` for zero divisors, and `ValueError` for undefined words or malformed definitions. Then check with a broad inline suite.'''
CONTENT = ""
CALLS = [
    {
        "name": "write",
        "arguments": {
            "path": "forth.py",
            "overwrite": True,
            "content": r'''class StackUnderflowError(Exception):
    def __init__(self, message):
        super().__init__(message)
        self.message = message


class Forth:
    def __init__(self):
        self.stack = []
        self.words = {}

    def evaluate(self, input_data):
        tokens = input_data.split()
        index = 0
        while index < len(tokens):
            token = tokens[index]
            if token == ":":
                index = self._define(tokens, index)
            else:
                self._run(token)
            index += 1
        return self.stack

    def _define(self, tokens, start):
        try:
            end = tokens.index(";", start + 1)
        except ValueError:
            raise ValueError("unterminated definition") from None
        if end == start + 1:
            raise ValueError("missing word name")
        name = tokens[start + 1].lower()
        self.words[name] = [token.lower() for token in tokens[start + 2:end]]
        return end

    def _run(self, token):
        word = token.lower()
        if _is_number(word):
            self.stack.append(int(word))
        elif word in self.words:
            for inner in list(self.words[word]):
                self._run(inner)
        elif word in self.OPERATORS:
            self.OPERATORS[word](self)
        else:
            raise ValueError(f"undefined operation: {token}")

    def _need(self, count):
        if len(self.stack) < count:
            raise StackUnderflowError("Insufficient number of items in stack")

    def _add(self):
        self._need(2)
        right = self.stack.pop()
        self.stack[-1] += right

    def _subtract(self):
        self._need(2)
        right = self.stack.pop()
        self.stack[-1] -= right

    def _multiply(self):
        self._need(2)
        right = self.stack.pop()
        self.stack[-1] *= right

    def _divide(self):
        self._need(2)
        right = self.stack.pop()
        if right == 0:
            self.stack.append(right)
            raise ZeroDivisionError("divide by zero")
        left = self.stack.pop()
        quotient = abs(left) // abs(right)
        if (left < 0) != (right < 0):
            quotient = -quotient
        self.stack.append(quotient)

    def _dup(self):
        self._need(1)
        self.stack.append(self.stack[-1])

    def _drop(self):
        self._need(1)
        self.stack.pop()

    def _swap(self):
        self._need(2)
        self.stack[-1], self.stack[-2] = self.stack[-2], self.stack[-1]

    def _over(self):
        self._need(2)
        self.stack.append(self.stack[-2])

    OPERATORS = {
        "+": _add,
        "-": _subtract,
        "*": _multiply,
        "/": _divide,
        "dup": _dup,
        "drop": _drop,
        "swap": _swap,
        "over": _over,
    }


def _is_number(token):
    digits = token[1:] if token.startswith("-") else token
    return bool(digits) and digits.isdigit() and digits.isascii()


def evaluate(input_data):
    return Forth().evaluate(input_data)
''',
        },
    },
    {
        "name": "shell",
        "arguments": {
            "cmd": r'''python3 - <<'PY'
from forth import StackUnderflowError, evaluate

def eq(source, want):
    got = evaluate(source)
    assert got == want, (source, got, want)

eq("", [])
eq("1 2 3 4 5", [1, 2, 3, 4, 5])
eq("-1 -2", [-1, -2])
eq("1 2 +", [3])
eq("5 3 -", [2])
eq("-5 3 *", [-15])
eq("10 2 /", [5])
eq("8 3 /", [2])
eq("-8 3 /", [-2])
eq("1 dup", [1, 1])
eq("1 2 drop", [1])
eq("1 2 swap", [2, 1])
eq("1 2 over", [1, 2, 1])
eq(": dup-twice dup dup ; 1 dup-twice", [1, 1, 1])
eq(": countup 1 2 3 ; : countup-twice countup countup ; countup-twice", [1, 2, 3, 1, 2, 3])
eq(": foo 1 ; : foo 2 ; foo", [2])
eq(": DUP 2 ; 1 dup", [1, 2])
eq(": foo bar ; : bar 1 ; foo", [1])
eq(": foo 1 ; : bar foo foo ; bar", [1, 1])
eq(": add2 2 + ; 3 add2 add2", [7])

def fails(source, exc, has=None):
    try:
        evaluate(source)
    except exc as error:
        assert str(error), "error message must not be empty"
        if has is not None:
            assert has in str(error), (source, str(error))
        return
    raise AssertionError("expected %s for %r" % (exc.__name__, source))

fails("+", StackUnderflowError)
fails("dup", StackUnderflowError)
fails("1 +", StackUnderflowError)
fails("1 swap", StackUnderflowError)
fails("1 over", StackUnderflowError)
fails("1 0 /", ZeroDivisionError, "divide by zero")
fails("foo", ValueError)
fails(": foo 1", ValueError)
fails(": ;", ValueError)

try:
    evaluate("1 0 /")
except ZeroDivisionError:
    pass

print("ALL PASS")
PY''',
            "expect_exit": "0",
            "expect_has": "ALL PASS",
        },
    },
    {
        "name": "finish",
        "arguments": {
            "summary": r'''Implemented the Forth evaluator in forth.py: `evaluate(input_data)` tokenizes the input, stores `: name ... ;` definitions as lowercased token lists (so words are case-insensitive and may call earlier definitions or override built-ins), and executes integers, `+ - * /` (integer division truncating toward zero, `ZeroDivisionError` on zero divisors), `DUP DROP SWAP OVER` and user words. Stack underflow raises `StackUnderflowError` with a message before the stack is changed, undefined words raise `ValueError`, as do unterminated definitions.

Verified with an inline suite: arithmetic, truncating division, all stack operators, user words, nested and redefined words, overriding a built-in in another case, forward-referenced words resolved at call time, plus every error path. All passed.'''
        },
    },
]

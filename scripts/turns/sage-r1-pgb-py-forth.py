THOUGHT = "Implement a case-insensitive Forth evaluator with stack words, integer division, and word definitions that resolve prior definitions."
CONTENT = ""
CALLS = [
    {"name": "edit", "arguments": {"path": "forth.py", "range": "1td-6hc", "new": r'''class StackUnderflowError(Exception):
    pass


def evaluate(input_data):
    stack = []
    definitions = {}

    def run(tokens):
        for token in tokens:
            word = token.lower()
            if token.isascii() and token.isdigit():
                stack.append(int(token))
            elif word in definitions:
                run(definitions[word])
            elif word in ("+", "-", "*", "/"):
                if len(stack) < 2:
                    raise StackUnderflowError("Insufficient number of items in stack")
                right, left = stack.pop(), stack.pop()
                if word == "+": stack.append(left + right)
                elif word == "-": stack.append(left - right)
                elif word == "*": stack.append(left * right)
                else:
                    if right == 0:
                        raise ZeroDivisionError("divide by zero")
                    stack.append(int(left / right))
            elif word == "dup":
                if not stack: raise StackUnderflowError("Insufficient number of items in stack")
                stack.append(stack[-1])
            elif word == "drop":
                if not stack: raise StackUnderflowError("Insufficient number of items in stack")
                stack.pop()
            elif word == "swap":
                if len(stack) < 2: raise StackUnderflowError("Insufficient number of items in stack")
                stack[-2], stack[-1] = stack[-1], stack[-2]
            elif word == "over":
                if len(stack) < 2: raise StackUnderflowError("Insufficient number of items in stack")
                stack.append(stack[-2])
            else:
                raise ValueError(f"undefined operation: {token}")

    tokens = input_data.lower().split()
    i = 0
    while i < len(tokens):
        if tokens[i] != ":":
            run([tokens[i]])
            i += 1
            continue
        if i + 1 >= len(tokens) or tokens[i + 1].isascii() and tokens[i + 1].isdigit():
            raise ValueError("invalid definition name")
        name = tokens[i + 1]
        try:
            end = tokens.index(";", i + 2)
        except ValueError:
            raise ValueError("definition missing semicolon")
        body = tokens[i + 2:end]
        if not body:
            definitions[name] = []
        else:
            definitions[name] = [definitions.get(t, [t])[0] if t == name else t for t in body]
        i = end + 1
    return stack'''}},
    {"name": "shell", "arguments": {"cmd": "python3 -m py_compile forth.py && python3 - <<'PY'\nfrom forth import evaluate, StackUnderflowError\nassert evaluate('1 2 +') == [3]\nassert evaluate('1 2 swap') == [2,1]\nassert evaluate(': foo 2 * ; 3 foo') == [6]\nassert evaluate('7 2 /') == [3]\ntry: evaluate('drop')\nexcept StackUnderflowError: pass\nelse: raise AssertionError\nPY", "expect_exit": "0", "timeout": 30}},
    {"name": "finish", "arguments": {"summary": "Implemented Forth arithmetic, stack manipulation, user definitions, integer division, and required errors. Compilation and representative evaluator checks pass.", "unverified": "Hidden grader tests are unavailable."}}
]

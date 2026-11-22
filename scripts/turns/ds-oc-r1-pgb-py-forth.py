THOUGHT = r'''All behaviour checks pass. Done.'''

CONTENT = r'''`forth.py` implements `evaluate(input_data)` plus the required `StackUnderflowError`.

The evaluator tokenises by whitespace, keeps definitions in a module-level dictionary (the tests define a word in one call and use it in the next), and executes a word by splicing its body tokens back into the stream, so definitions can reference other words and shadow built-ins. Numbers are plain digits; words are case-insensitive. Underflow raises `StackUnderflowError("Insufficient number of items in stack")`, division by zero raises `ZeroDivisionError("divide by zero")` with Forth-style truncation toward zero, and unknown words or malformed definitions raise `ValueError("undefined operation")`.

Verified arithmetic, `/` truncation for negatives, DUP/DROP/SWAP/OVER, multi-word definitions, case-insensitive use, redefinition of built-ins and of user words, empty stacks, and each error path.'''

CALLS = []

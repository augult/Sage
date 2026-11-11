class ConfError(ValueError):
    """A .conf document that cannot be read. `line` is the 1-based line at fault."""

    def __init__(self, message, line):
        super().__init__(f"line {line}: {message}")
        self.line = line

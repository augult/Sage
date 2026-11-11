"""Reader for the .conf format. SPEC.md is the specification."""

from .errors import ConfError
from .parser import load, parse

__all__ = ["ConfError", "load", "parse"]

"""Ledger entries, one per CSV line: date,category,amount[,description]."""

import csv
from dataclasses import dataclass

from .money import parse_amount


@dataclass(frozen=True)
class Entry:
    date: str
    category: str
    cents: int
    description: str = ""


def parse_line(line: str) -> Entry:
    """Standard CSV quoting applies: a quoted description may contain commas and doubled quotes."""
    row = next(csv.reader([line.rstrip("\n")]))
    if len(row) < 3:
        raise ValueError(f"expected date,category,amount[,description]: {line!r}")
    date, category, amount = (field.strip() for field in row[:3])
    description = row[3].strip() if len(row) > 3 else ""
    return Entry(date, category.lower(), parse_amount(amount), description)


def parse_lines(lines) -> list[Entry]:
    """Skips blank lines and lines starting with '#'."""
    return [parse_line(line) for line in lines if line.strip() and not line.lstrip().startswith("#")]

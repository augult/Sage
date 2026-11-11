"""Month bucketing for ISO dates."""


def month_key(date: str) -> str:
    """'2024-03-17' -> '2024-03'."""
    year, month, _ = date.split("-")
    return f"{int(year):04d}-{int(month):02d}"


def month_range(start: str, end: str) -> list[str]:
    """Every month key from start to end inclusive: ('2024-01', '2024-03') -> ['2024-01', '2024-02', '2024-03']."""
    year, month = (int(part) for part in start.split("-"))
    end_year, end_month = (int(part) for part in end.split("-"))
    months = []
    while (year, month) <= (end_year, end_month):
        months.append(f"{year:04d}-{month:02d}")
        month += 1
        if month == 13:
            year, month = year + 1, 1
    return months

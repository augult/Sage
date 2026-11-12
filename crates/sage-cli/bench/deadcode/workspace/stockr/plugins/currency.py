"""Exporter: the report's value in another currency."""


def _convert(amount, rate):
    return round(amount * rate, 2)


def convert_totals(summary, rate=1.0):
    """A copy of the summary with `value` converted at the given rate."""
    converted = dict(summary)
    converted["value"] = _convert(summary["value"], rate)
    return converted

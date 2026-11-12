"""Report formats. `build_report` picks `format_<name>` for the format it is asked for."""

import csv
import io
import json
import re

from .util import money


def format_text(report):
    lines = [
        f"items: {report['items']}",
        f"units: {report['units']}",
        f"value: {money(report['value'])}",
        f"mean qty: {report['mean_qty']:.1f}",
        f"median qty: {report['median_qty']:.1f}",
    ]
    for name, sku, message in report["problems"]:
        lines.append(f"! {sku}: {message} [{name}]")
    return "\n".join(lines) + "\n"


def format_csv(report):
    out = io.StringIO()
    writer = csv.writer(out, lineterminator="\n")
    writer.writerow(["items", "units", "value", "mean_qty", "median_qty", "problems"])
    writer.writerow(
        [
            report["items"],
            report["units"],
            f"{report['value']:.2f}",
            f"{report['mean_qty']:.1f}",
            f"{report['median_qty']:.1f}",
            len(report["problems"]),
        ]
    )
    return out.getvalue()


def format_json(report):
    return json.dumps(report, indent=2, sort_keys=True) + "\n"


def _legacy_table(rows):
    widths = [max(len(str(cell)) for cell in column) for column in zip(*rows)]
    lines = []
    for row in rows:
        lines.append(" | ".join(str(cell).ljust(width) for cell, width in zip(row, widths)))
    return "\n".join(lines)

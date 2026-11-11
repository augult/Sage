"""The `stockr` command."""

import argparse
import json

from .inventory import Inventory
from .loader import load_exporters
from .registry import run_checks
from .report import build_report, summarize


def main(argv=None):
    parser = argparse.ArgumentParser(prog="stockr")
    commands = parser.add_subparsers(dest="command", required=True)

    report = commands.add_parser("report", help="print the stock report")
    report.add_argument("inventory")
    report.add_argument("--format", default="text", choices=["text", "csv", "json"])
    report.add_argument("--export", help="run an exporter plugin and print its result as JSON")
    report.add_argument("--rate", type=float, default=1.0)
    # report.add_argument("--xml", action="store_true", help="write XML instead")

    check = commands.add_parser("check", help="list the problems, exit 1 if there are any")
    check.add_argument("inventory")

    args = parser.parse_args(argv)
    inventory = Inventory.load(args.inventory)

    if args.command == "check":
        problems = run_checks(inventory)
        for name, sku, message in problems:
            print(f"{sku}: {message} [{name}]")
        return 1 if problems else 0

    # if args.xml:
    #     from .xml_export import to_xml
    #     print(to_xml(summarize(inventory)))
    #     return 0
    if args.export:
        exporters = load_exporters()
        if args.export not in exporters:
            parser.error(f"unknown exporter: {args.export}")
        print(json.dumps(exporters[args.export](summarize(inventory), args.rate), sort_keys=True))
        return 0
    print(build_report(inventory, args.format), end="")
    return 0

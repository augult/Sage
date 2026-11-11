"""`python3 -m fetchkit`: fetch URLs or a manifest, or show the effective settings."""

import argparse
import sys

from .client import Client
from .config import ConfigError, load_config, validate
from .jobs import Job, ManifestError, load_manifest, run_jobs


def build_parser():
    parser = argparse.ArgumentParser(prog="fetchkit", description="Download files, trying again when one fails.")
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument("--config", metavar="PATH", help="settings file (INI, section [fetch])")
    common.add_argument("--attempts", type=int, metavar="N", help="try a download up to N times (default: 3)")
    common.add_argument("--timeout", type=float, metavar="SECONDS", help="give up on one try after this long (default: 10)")
    commands = parser.add_subparsers(dest="command", required=True)
    fetch = commands.add_parser("fetch", parents=[common], help="download URLs")
    fetch.add_argument("--manifest", metavar="FILE", help="a JSON manifest of jobs")
    fetch.add_argument("urls", nargs="*", metavar="URL")
    commands.add_parser("show-config", parents=[common], help="print the effective settings")
    return parser


def effective_config(args):
    """Flags win over the environment, which wins over the config file."""
    config = load_config(args.config)
    if args.attempts is not None:
        config["attempts"] = args.attempts
    if args.timeout is not None:
        config["timeout"] = args.timeout
    return validate(config)


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        config = effective_config(args)
        if args.command == "show-config":
            for key in sorted(config):
                print(f"{key} = {config[key]}")
            return 0
        jobs = [Job(url) for url in args.urls]
        if args.manifest:
            jobs += load_manifest(args.manifest)
    except (ConfigError, ManifestError) as error:
        print(f"fetchkit: {error}", file=sys.stderr)
        return 2
    if not jobs:
        print("fetchkit: nothing to fetch", file=sys.stderr)
        return 2
    client = Client(attempts=config["attempts"], timeout=config["timeout"], backoff=config["backoff"], sleep=lambda _: None)
    failed = 0
    for url, tries, ok in run_jobs(jobs, client):
        if ok:
            print(f"ok {url} ({tries} tries)")
        else:
            failed += 1
            print(f"FAILED {url} after {tries} attempts")
    return 1 if failed else 0

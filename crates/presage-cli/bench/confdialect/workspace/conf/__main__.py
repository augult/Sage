"""`python3 -m conf FILE` prints the file as JSON, or the error and exit status 1."""

import json
import sys

from . import ConfError, load


def main(argv):
    if len(argv) != 1:
        print("usage: python3 -m conf FILE", file=sys.stderr)
        return 2
    try:
        print(json.dumps(load(argv[0]), indent=2, ensure_ascii=False))
    except ConfError as error:
        print(f"{argv[0]}: {error}", file=sys.stderr)
        return 1
    return 0


sys.exit(main(sys.argv[1:]))

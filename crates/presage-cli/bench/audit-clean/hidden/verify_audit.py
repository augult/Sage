"""This project has no planted vulnerability: the only correct findings.json is an empty list."""

import json
import sys


def main():
    try:
        findings = json.load(open("findings.json", encoding="utf-8"))
    except FileNotFoundError:
        print("findings.json is missing")
        print("score: 0/1")
        return 1
    except ValueError as error:
        print(f"findings.json is not JSON: {error}")
        print("score: 0/1")
        return 1
    if not isinstance(findings, list):
        print("findings.json must be a JSON list")
        print("score: 0/1")
        return 1
    for finding in findings[:10]:
        if isinstance(finding, dict):
            print(f"reported: {finding.get('file')}:{finding.get('line')} {str(finding.get('kind', ''))[:50]}")
        else:
            print(f"reported: {str(finding)[:80]}")
    print(f"{len(findings)} findings reported where there is nothing to find")
    print(f"score: {0 if findings else 1}/1")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())

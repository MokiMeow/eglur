"""Validate the v0.1 OPEN-item ledger against stable-ID OPEN documents."""

import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path

import yaml
from eglur_conformance.spec_release import open_items
from eglur_conformance.yaml_document import load_yaml


def build_parser() -> ArgumentParser:
    """Build the CLI parser."""
    parser = ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="repository root (default: current directory)",
    )
    parser.add_argument(
        "--ledger",
        type=Path,
        default=open_items.DEFAULT_LEDGER_PATH,
        help=(
            "ledger path relative to repo root (default: "
            f"{open_items.DEFAULT_LEDGER_PATH})"
        ),
    )
    return parser


def run(args: Namespace) -> int:
    """Execute validation and return a process status code."""
    repo_root = args.repo_root.resolve()
    ledger_path = args.ledger
    if not ledger_path.is_absolute():
        ledger_path = repo_root / ledger_path

    try:
        ledger = open_items.decode_ledger(load_yaml(ledger_path), str(ledger_path))
        inspection = open_items.inspect_sources(repo_root, ledger.sources)
        problems = open_items.validate_ledger(ledger, inspection)
    except (OSError, TypeError, ValueError, yaml.YAMLError) as error:
        print(f"OPEN ledger validation failed: {error}", file=sys.stderr)
        return 1

    if problems:
        print("OPEN ledger validation failed:", file=sys.stderr)
        for problem in problems:
            print(f"- {problem.message}", file=sys.stderr)
        return 1

    source_count = len(ledger.sources)
    print(
        "OPEN ledger validation passed: "
        f"{len(ledger.items)} items across {source_count} source files."
    )
    return 0


def main() -> int:
    """CLI entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

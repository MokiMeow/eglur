"""Validate the trusted standard-library effect-summary catalog."""

import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path

import yaml
from eglur_conformance.effects import codec


def build_parser() -> ArgumentParser:
    """Build the command-line parser."""
    parser = ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="repository root (default: current directory)",
    )
    parser.add_argument(
        "--catalog",
        type=Path,
        default=codec.DEFAULT_CATALOG_PATH,
        help=f"catalog path (default: {codec.DEFAULT_CATALOG_PATH})",
    )
    return parser


def run(args: Namespace) -> int:
    """Validate the catalog and return a process status code."""
    try:
        catalog = codec.load_effect_catalog(args.repo_root.resolve(), args.catalog)
    except (OSError, TypeError, ValueError, yaml.YAMLError) as error:
        print(f"effect-summary catalog validation failed: {error}", file=sys.stderr)
        return 1
    print(
        "effect-summary catalog validation passed: "
        f"{len(catalog.callables)} callables, {len(catalog.aliases)} aliases"
    )
    return 0


def main() -> int:
    """Run the command-line entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

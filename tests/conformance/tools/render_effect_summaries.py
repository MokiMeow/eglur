"""Render stdlib effect-summary reference Markdown from canonical metadata."""

import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path

import yaml
from eglur_conformance.effects import codec, render


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
    """Render validated reference Markdown to standard output."""
    try:
        catalog = codec.load_effect_catalog(args.repo_root.resolve(), args.catalog)
    except (OSError, TypeError, ValueError, yaml.YAMLError) as error:
        print(f"cannot render effect-summary reference: {error}", file=sys.stderr)
        return 1
    print(render.render_markdown(catalog), end="")
    return 0


def main() -> int:
    """Run the command-line entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

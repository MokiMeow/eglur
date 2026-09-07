"""Generate or verify the first semantic fixture corpus."""

import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path

import yaml
from eglur_conformance.semantic_slice import repository


def build_parser() -> ArgumentParser:
    """Build the command-line parser."""
    parser = ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="Git worktree root (default: current directory)",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help=f"fail if {repository.CORPUS_PATH} is not byte-exact",
    )
    return parser


def run(args: Namespace) -> int:
    """Generate the corpus or check exact freshness."""
    try:
        if args.check:
            if not repository.corpus_is_current(args.repo_root):
                print(
                    "semantic fixture freshness check failed: "
                    f"{repository.CORPUS_PATH} is missing or stale; "
                    "run pixi -q run --locked generate-semantic-expectations",
                    file=sys.stderr,
                )
                return 1
            print("semantic fixture freshness check passed")
            return 0

        repository.write_corpus(args.repo_root)
    except (OSError, TypeError, UnicodeError, ValueError, yaml.YAMLError) as error:
        print(f"semantic fixture projection failed: {error}", file=sys.stderr)
        return 1
    print(f"wrote {repository.CORPUS_PATH}")
    return 0


def main() -> int:
    """Run the command-line entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

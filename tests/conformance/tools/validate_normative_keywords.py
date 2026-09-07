"""Reject lowercase RFC modal keywords in bounded normative tensor prose."""

from argparse import ArgumentParser, Namespace
from pathlib import Path

from eglur_conformance.spec_release import normative_keywords


def build_parser() -> ArgumentParser:
    """Build the CLI parser."""
    parser = ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="repository root (default: current directory)",
    )
    return parser


def run(args: Namespace) -> int:
    """Execute the bounded keyword audit."""
    violations = normative_keywords.find_violations(args.repo_root.resolve())
    if violations:
        print("normative keyword validation failed:")
        for violation in violations:
            print(f"- {violation.render()}")
        return 1

    print("normative keyword validation passed")
    return 0


def main() -> int:
    """CLI entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

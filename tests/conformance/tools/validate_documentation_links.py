"""Validate local links across every tracked Markdown document."""

from argparse import ArgumentParser, Namespace
from pathlib import Path

from eglur_conformance.documentation import links, repository


def build_parser() -> ArgumentParser:
    """Build the command-line parser.

    Returns
    -------
    argparse.ArgumentParser
        Parser for the repository-root boundary.
    """
    parser = ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="repository root (default: current directory)",
    )
    return parser


def run(args: Namespace) -> int:
    """Validate the tracked documentation tree.

    Parameters
    ----------
    args : argparse.Namespace
        Parsed command-line arguments containing ``repo_root``.

    Returns
    -------
    int
        Zero after successful validation, otherwise one.
    """
    try:
        tracked = repository.load_tracked_documentation(args.repo_root)
        violations = links.find_violations(tracked.sources, tracked.targets)
    except (OSError, ValueError) as error:
        print("documentation link validation failed:")
        print(f"- {error}")
        return 1

    if violations:
        print("documentation link validation failed:")
        for violation in violations:
            print(f"- {violation.render()}")
        return 1

    print(
        f"documentation link validation passed: {len(tracked.sources)} Markdown files."
    )
    return 0


def main() -> int:
    """Run the command-line entry point.

    Returns
    -------
    int
        Process exit status.
    """
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

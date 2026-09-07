"""Validate source suites and implementation-probe schema invariants."""

import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path

import yaml
from eglur_conformance.registry_cache import repository as registry_cache_repository
from eglur_conformance.suite import schema


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
        "--manifest",
        type=Path,
        default=schema.DEFAULT_MANIFEST_PATH,
        help=(
            "manifest path relative to repo root "
            f"(default: {schema.DEFAULT_MANIFEST_PATH})"
        ),
    )
    return parser


def run(args: Namespace) -> int:
    """Execute validation and return a process status code."""
    repo_root = args.repo_root.resolve()
    manifest_path = args.manifest
    if not manifest_path.is_absolute():
        manifest_path = repo_root / manifest_path

    try:
        problems = schema.validate_all(repo_root, manifest_path)
        problems.extend(
            registry_cache_repository.validate_repository(
                repo_root,
                manifest_path,
            )
        )
    except (OSError, TypeError, ValueError, yaml.YAMLError) as error:
        print(f"conformance schema validation failed: {error}", file=sys.stderr)
        return 1

    if problems:
        print("conformance schema validation failed:", file=sys.stderr)
        for problem in problems:
            print(f"- {problem.message}", file=sys.stderr)
        return 1

    print("conformance schema validation passed")
    return 0


def main() -> int:
    """CLI entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

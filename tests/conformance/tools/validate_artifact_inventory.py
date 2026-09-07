"""Validate the v0.1 authoritative artifact inventory."""

import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path

import yaml
from eglur_conformance.spec_release import artifact_inventory
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
        "--inventory",
        type=Path,
        default=artifact_inventory.DEFAULT_INVENTORY_PATH,
        help=(f"inventory path (default: {artifact_inventory.DEFAULT_INVENTORY_PATH})"),
    )
    return parser


def run(args: Namespace) -> int:
    """Validate the configured repository inventory."""
    repo_root = args.repo_root.resolve()
    inventory_path = (
        args.inventory if args.inventory.is_absolute() else repo_root / args.inventory
    )
    try:
        inventory = artifact_inventory.decode_inventory(
            load_yaml(inventory_path),
            str(inventory_path),
        )
        candidates = artifact_inventory.discover_authoritative_artifacts(repo_root)
        problems = artifact_inventory.validate_checkout_binding(repo_root)
        problems.extend(artifact_inventory.validate_inventory(inventory, candidates))
    except (OSError, TypeError, ValueError, yaml.YAMLError) as error:
        print(f"artifact inventory validation failed: {error}", file=sys.stderr)
        return 1

    if problems:
        print("artifact inventory validation failed:", file=sys.stderr)
        for problem in problems:
            print(f"- {problem.message}", file=sys.stderr)
        return 1

    print(f"artifact inventory validation passed: {len(candidates)} artifacts.")
    return 0


def main() -> int:
    """CLI entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

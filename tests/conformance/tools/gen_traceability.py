"""Generate machine-checkable diagnostic-code traceability for conformance tests."""

import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path

from eglur_conformance.traceability import codec, repository


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
        "--check",
        action="store_true",
        help="fail if tests/conformance/traceability.yaml is not up to date",
    )
    return parser


def run(args: Namespace) -> int:
    """Execute generation or byte-exact freshness checking."""
    repo_root = args.repo_root.resolve()
    out_path = repo_root / repository.TRACEABILITY_PATH
    report = repository.build_report(repo_root)
    generated = codec.encode_report(report)

    if args.check:
        try:
            current = out_path.read_bytes()
        except OSError as error:
            print(f"traceability freshness check failed: {error}", file=sys.stderr)
            return 1
        if current != generated:
            print(
                "traceability freshness check failed: "
                f"{out_path.relative_to(repo_root)} is stale; "
                "run pixi -q run --locked generate-traceability",
                file=sys.stderr,
            )
            return 1
        if report.contract_failures:
            print(
                "diagnostic contract validation failed: "
                f"{len(report.contract_failures)} missing assertions",
                file=sys.stderr,
            )
            return 1
        print("traceability freshness check passed")
        return 0

    out_path.write_bytes(generated)
    if report.contract_failures:
        print(
            "diagnostic contract validation failed: "
            f"{len(report.contract_failures)} missing assertions",
            file=sys.stderr,
        )
        return 1
    return 0


def main() -> int:
    """CLI entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

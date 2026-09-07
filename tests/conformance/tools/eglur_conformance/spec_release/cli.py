"""Validate Eglur v0.1 spec evidence and certification availability."""

import sys
from argparse import ArgumentParser, Namespace
from collections.abc import Sequence
from pathlib import Path

import yaml

from eglur_conformance.spec_release import gate
from eglur_conformance.validation import ValidationProblem


def build_parser() -> ArgumentParser:
    """Build the CLI parser."""
    parser = ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="verdict_kind", required=True)

    spec_parser = subparsers.add_parser(
        "spec-package",
        help="validate spec-package release readiness",
    )
    spec_parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="repository root (default: current directory)",
    )
    spec_parser.add_argument(
        "--signoff",
        type=Path,
        required=True,
        help="YAML spec-package release sign-off artifact",
    )

    subparsers.add_parser(
        "implementation",
        help="report fail-closed implementation certification availability",
    )
    return parser


def report_failure(
    verdict_label: str,
    problems: Sequence[ValidationProblem],
) -> int:
    """Print one explicitly scoped negative verdict."""
    print(f"{verdict_label}:", file=sys.stderr)
    for problem in problems:
        print(f"- {problem.message}", file=sys.stderr)
    return 1


def report_success(
    verdict_label: str,
    checked: Sequence[str],
) -> int:
    """Print one explicitly scoped positive verdict."""
    print(verdict_label)
    print("checked:")
    for item in checked:
        print(f"- {item}")
    return 0


def run_spec_package(args: Namespace) -> int:
    """Validate spec-package release readiness."""
    repo_root = args.repo_root.resolve()
    signoff_path = args.signoff
    if not signoff_path.is_absolute():
        signoff_path = repo_root / signoff_path

    try:
        evaluation = gate.evaluate_spec_package(
            repo_root,
            signoff_path,
            str(args.signoff),
        )
    except (OSError, TypeError, ValueError, yaml.YAMLError) as error:
        print(f"spec-package release readiness: NO-GO: {error}", file=sys.stderr)
        return 1

    if not evaluation.ready:
        return report_failure(
            "spec-package release readiness: NO-GO",
            evaluation.problems,
        )

    return report_success(
        "spec-package release readiness: GO",
        evaluation.checked,
    )


def run_implementation(_args: Namespace) -> int:
    """Refuse certification until a complete executable-evidence profile exists."""
    print("implementation conformance: UNAVAILABLE", file=sys.stderr)
    print(
        "- no released executable-evidence profile can support PASS",
        file=sys.stderr,
    )
    return 2


def run(args: Namespace) -> int:
    """Execute the explicitly selected verdict validation."""
    if args.verdict_kind == "spec-package":
        return run_spec_package(args)
    if args.verdict_kind == "implementation":
        return run_implementation(args)
    raise ValueError(f"unsupported verdict kind: {args.verdict_kind}")


def main() -> int:
    """CLI entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

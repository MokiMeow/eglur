"""Report release and certification availability without issuing verdicts."""

import sys
from argparse import ArgumentParser, Namespace


def build_parser() -> ArgumentParser:
    """Build the CLI parser."""
    parser = ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="verdict_kind", required=True)
    subparsers.add_parser(
        "spec-package",
        help="report unavailable standalone specification releases",
    )
    subparsers.add_parser(
        "implementation",
        help="report unavailable implementation certification",
    )
    return parser


def run(args: Namespace) -> int:
    """Report the selected unavailable operation on stderr with exit status 2."""
    if args.verdict_kind == "spec-package":
        print("spec-package release readiness: UNAVAILABLE", file=sys.stderr)
        print(
            "- specifications are developed toward the integrated Eglur release; "
            "no standalone specification release is supported",
            file=sys.stderr,
        )
        return 2
    if args.verdict_kind == "implementation":
        print("implementation conformance: UNAVAILABLE", file=sys.stderr)
        print(
            "- no released executable-evidence profile can support PASS",
            file=sys.stderr,
        )
        return 2
    raise ValueError(f"unsupported verdict kind: {args.verdict_kind}")


def main() -> int:
    """CLI entry point."""
    return run(build_parser().parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

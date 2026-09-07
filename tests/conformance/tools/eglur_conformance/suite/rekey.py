"""Rekey conformance test IDs to semantic title-derived identifiers.

This tool rewrites suite-local `- id:` entries from short numeric IDs such as
`TYC-034` into semantic IDs such as
`TYC-property-assign-forbidden`.

It can also rewrite non-generated repository references that mention those test
IDs directly. `traceability.yaml` remains generated output and should be
regenerated separately after applying a rewrite.
"""

import re
import sys
import unicodedata
from argparse import ArgumentParser, Namespace
from dataclasses import dataclass
from pathlib import Path

import yaml
from yaml.nodes import MappingNode, Node, ScalarNode, SequenceNode

from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.yaml_document import load_yaml

RE_NON_ALNUM = re.compile(r"[^a-z0-9]+")
RE_HYPHEN_RUN = re.compile(r"-+")

GENERATED_TRACEABILITY = Path("tests/conformance/traceability.yaml")
EXCLUDED_SUITE_FILES = {"manifest.yaml", "traceability.yaml"}
TEXT_FILE_SUFFIXES = {
    ".md",
    ".py",
    ".txt",
    ".toml",
    ".yaml",
    ".yml",
}
EXCLUDED_REWRITE_PATHS = {
    Path(".git"),
    Path(".tickets"),
    Path("AGENTS.md"),
    GENERATED_TRACEABILITY,
    Path("tests/conformance/tools/rekey_ids.py"),
    Path("tests/conformance/tools/eglur_conformance/suite/rekey.py"),
}


@dataclass(frozen=True)
class TestEntry:
    """One discovered test entry."""

    file_path: Path
    old_id: str
    prefix: str
    title: str


def slugify_title(title: str) -> str:
    """Return a deterministic ASCII slug derived from one test title."""
    normalized = unicodedata.normalize("NFKD", title)
    ascii_text = normalized.encode("ascii", "ignore").decode("ascii")
    lowered = ascii_text.lower()
    compact = RE_NON_ALNUM.sub("-", lowered)
    collapsed = RE_HYPHEN_RUN.sub("-", compact).strip("-")
    if not collapsed:
        raise ValueError(f"title does not produce a non-empty slug: {title!r}")
    return collapsed


def discover_suite_files(paths: list[Path]) -> list[Path]:
    """Return sorted suite YAML files under the provided paths."""
    discovered: list[Path] = []
    for path in paths:
        if path.is_file():
            if path.name not in EXCLUDED_SUITE_FILES and path.suffix in {
                ".yaml",
                ".yml",
            }:
                discovered.append(path)
            continue
        for candidate in sorted(path.rglob("*.yaml")):
            if candidate.name in EXCLUDED_SUITE_FILES:
                continue
            discovered.append(candidate)
    return sorted(dict.fromkeys(discovered))


def _manifest_suite_selection(repo_root: Path) -> tuple[Path, list[Path]]:
    manifest = suite_schema.parse_manifest(
        repo_root / suite_schema.DEFAULT_MANIFEST_PATH
    )
    suite_root = repo_root / manifest.root
    suite_files = sorted(suite_root / entry.file for entry in manifest.suites)
    return suite_root, suite_files


def collect_entries(file_path: Path) -> list[TestEntry]:
    """Collect test IDs and titles from one suite file."""
    raw = load_yaml(file_path)
    if not isinstance(raw, dict):
        raise TypeError(f"{file_path} must contain a mapping")
    raw_tests = raw.get("tests")
    if not isinstance(raw_tests, list):
        raise TypeError(f"{file_path}.tests must be a list")

    entries: list[TestEntry] = []
    for index, raw_test in enumerate(raw_tests):
        if not isinstance(raw_test, dict):
            raise TypeError(f"{file_path}.tests[{index}] must be a mapping")
        old_id = raw_test.get("id")
        title = raw_test.get("title")
        if not isinstance(old_id, str) or not old_id:
            raise ValueError(
                f"{file_path}.tests[{index}].id must be a non-empty string"
            )
        if not isinstance(title, str) or not title:
            raise ValueError(
                f"{file_path}.tests[{index}].title must be a non-empty string"
            )
        prefix, separator, _rest = old_id.partition("-")
        if not prefix or not separator:
            raise ValueError(f"unsupported test id format in {file_path}: {old_id}")
        entries.append(
            TestEntry(
                file_path=file_path,
                old_id=old_id,
                prefix=prefix,
                title=title,
            )
        )
    return entries


def assign_new_ids(entries: list[TestEntry]) -> dict[str, str]:
    """Assign globally unique semantic IDs keyed by old ID."""
    mapping: dict[str, str] = {}
    used: set[str] = set()
    for entry in entries:
        if entry.old_id in mapping:
            raise ValueError(f"duplicate test id: {entry.old_id}")
        base = f"{entry.prefix}-{slugify_title(entry.title)}"
        candidate = base
        suffix = 2
        while candidate in used:
            candidate = f"{base}-{suffix}"
            suffix += 1
        used.add(candidate)
        mapping[entry.old_id] = candidate
    return mapping


def mapping_value(node: MappingNode, key: str) -> Node | None:
    """Return one scalar-keyed YAML mapping value."""
    for key_node, value_node in node.value:
        if isinstance(key_node, ScalarNode) and key_node.value == key:
            return value_node
    return None


def render_id_scalar(value: str, style: str | None) -> str:
    """Render a semantic ID while preserving ordinary YAML quote style."""
    if style == "'":
        return f"'{value}'"
    if style == '"':
        return f'"{value}"'
    return value


def rewrite_suite_file(file_path: Path, mapping: dict[str, str]) -> bool:
    """Rewrite structured `tests[].id` scalar nodes without reformatting YAML."""
    source = file_path.read_text(encoding="utf-8")
    root = yaml.compose(source)
    if not isinstance(root, MappingNode):
        raise TypeError(f"{file_path} must contain a mapping")
    tests_node = mapping_value(root, "tests")
    if not isinstance(tests_node, SequenceNode):
        raise TypeError(f"{file_path}.tests must be a list")

    replacements: list[tuple[int, int, str]] = []
    for index, test_node in enumerate(tests_node.value):
        if not isinstance(test_node, MappingNode):
            raise TypeError(f"{file_path}.tests[{index}] must be a mapping")
        id_node = mapping_value(test_node, "id")
        if not isinstance(id_node, ScalarNode):
            raise TypeError(f"{file_path}.tests[{index}].id must be a scalar")
        replacement = mapping.get(id_node.value)
        if replacement is None or replacement == id_node.value:
            continue
        replacements.append(
            (
                id_node.start_mark.index,
                id_node.end_mark.index,
                render_id_scalar(replacement, id_node.style),
            )
        )

    if not replacements:
        return False
    for start, end, replacement in reversed(replacements):
        source = f"{source[:start]}{replacement}{source[end:]}"
    file_path.write_text(source, encoding="utf-8")
    return True


def should_skip_rewrite(path: Path) -> bool:
    """Return True when a path should not participate in reference rewriting."""
    for excluded in EXCLUDED_REWRITE_PATHS:
        if path == excluded or excluded in path.parents:
            return True
    return False


def candidate_text_files(repo_root: Path) -> list[Path]:
    """Return repo text files eligible for reference rewriting."""
    files: list[Path] = []
    for path in repo_root.rglob("*"):
        if not path.is_file():
            continue
        rel_path = path.relative_to(repo_root)
        if should_skip_rewrite(rel_path):
            continue
        if path.suffix not in TEXT_FILE_SUFFIXES:
            continue
        files.append(path)
    return sorted(files)


def rewrite_references(repo_root: Path, mapping: dict[str, str]) -> list[Path]:
    """Rewrite exact token references across text files. Returns changed paths."""
    if not mapping:
        return []
    old_ids = sorted(mapping, key=len, reverse=True)
    pattern = re.compile(
        r"(?<![A-Za-z0-9_-])("
        + "|".join(re.escape(old_id) for old_id in old_ids)
        + r")(?![A-Za-z0-9_-])"
    )
    changed_paths: list[Path] = []
    for file_path in candidate_text_files(repo_root):
        original = file_path.read_text()
        updated = pattern.sub(lambda match: mapping[match.group(1)], original)
        if updated != original:
            file_path.write_text(updated)
            changed_paths.append(file_path)
    return changed_paths


def build_parser() -> ArgumentParser:
    """Build the CLI parser."""
    parser = ArgumentParser(description=__doc__)
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        default=[],
        help=(
            "suite files or directories to rekey (default: manifest-owned suite files)"
        ),
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path("."),
        help="repository root for optional reference rewriting",
    )
    parser.add_argument(
        "--rewrite-refs",
        action="store_true",
        help="rewrite non-generated repository references that mention old IDs",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="apply changes in place; default is dry-run preview",
    )
    return parser


def run(args: Namespace) -> int:
    """Execute the CLI command."""
    repo_root = args.repo_root.resolve()
    if args.paths:
        input_paths = [
            path if path.is_absolute() else repo_root / path for path in args.paths
        ]
        canonical_suite_root = (
            repo_root / suite_schema.DEFAULT_MANIFEST_PATH.parent
        ).resolve()
        manifest_selection: tuple[Path, list[Path]] | None = None
        suite_files = []
        for path in input_paths:
            if path.resolve() == canonical_suite_root:
                if manifest_selection is None:
                    manifest_selection = _manifest_suite_selection(repo_root)
                _manifest_root, manifest_suite_files = manifest_selection
                suite_files.extend(manifest_suite_files)
            else:
                suite_files.extend(discover_suite_files([path]))
        suite_files = sorted(dict.fromkeys(suite_files))
    else:
        _manifest_root, manifest_suite_files = _manifest_suite_selection(repo_root)
        suite_files = manifest_suite_files
    if not suite_files:
        print("rekey failed: no suite files found", file=sys.stderr)
        return 2

    entries: list[TestEntry] = []
    for suite_file in suite_files:
        entries.extend(collect_entries(suite_file))
    if not entries:
        print("rekey failed: suite files contain no tests", file=sys.stderr)
        return 2
    mapping = assign_new_ids(entries)

    for entry in entries:
        print(f"{entry.old_id} -> {mapping[entry.old_id]}  [{entry.file_path}]")

    if not args.write:
        print("\nDry run only. Re-run with --write to apply changes.")
        return 0

    changed_suite_files = [
        path for path in suite_files if rewrite_suite_file(path, mapping)
    ]
    print(f"\nRewrote {len(changed_suite_files)} suite files.")

    if args.rewrite_refs:
        changed_ref_files = rewrite_references(repo_root, mapping)
        print(f"Rewrote {len(changed_ref_files)} additional repository files.")
        print("Regenerate tests/conformance/traceability.yaml after applying rewrites.")

    return 0


def main() -> int:
    """CLI entry point."""
    parser = build_parser()
    return run(parser.parse_args())


if __name__ == "__main__":
    raise SystemExit(main())

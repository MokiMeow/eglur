"""Canonical OPEN-item ledger, source inspection, and validation."""

import re
from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from eglur_conformance.markdown_document import visible_markdown_lines
from eglur_conformance.release_identity import (
    parse_release_label,
    validate_history_root,
)
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue

DEFAULT_LEDGER_PATH = Path("docs/spec/open-items-ledger-v0.1.yaml")
OPEN_ITEM_ID_PATTERN = r"[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)+"
RE_OPEN_ITEM_ID = re.compile(rf"^{OPEN_ITEM_ID_PATTERN}$")
RE_TOP_LEVEL_NUMBERED_ITEM = re.compile(r"^([1-9][0-9]*)\.(?:\s|$)")
RE_TOP_LEVEL_OPEN_ITEM = re.compile(
    rf"^([1-9][0-9]*)\.\s+\*\*`({OPEN_ITEM_ID_PATTERN})`\*\*\s+-\s+(.+?)\s*$"
)
SUMMARY_KEYS = {
    "total_items",
    "baseline_must_close_count",
    "deferred_nonbaseline_count",
    "unclassified_count",
    "baseline_must_close_open_count",
}
LEDGER_KEYS = {
    "version",
    "release_history_root",
    "release",
    "status",
    "purpose",
    "classification_keys",
    "sources",
    "items",
    "summary",
}
SOURCE_KEYS = {"path", "item_count"}
ITEM_KEYS = {"id", "source_path", "classification"}
SUPPORTED_VERSION = "0.1"
ALLOWED_STATUSES = {"draft", "released"}


@dataclass(frozen=True)
class SourceConfig:
    """One source document declared by the ledger."""

    path: str
    item_count: int


@dataclass(frozen=True)
class SourceItem:
    """One stable-ID OPEN item extracted from a top-level Markdown list."""

    path: str
    number: int
    item_id: str
    title: str


@dataclass(frozen=True)
class LedgerItem:
    """One ledger classification entry."""

    item_id: str
    source_path: str
    classification: str


@dataclass(frozen=True)
class Ledger:
    """Parsed OPEN-item ledger with narrowed field types."""

    version: str
    release_history_root: str
    release: str
    status: str
    purpose: str
    classification_keys: tuple[str, ...]
    sources: tuple[SourceConfig, ...]
    items: tuple[LedgerItem, ...]
    summary: dict[str, int]


@dataclass(frozen=True)
class SourceInspection:
    """Canonical source items plus boundary problems found while loading them."""

    items_by_path: tuple[tuple[str, tuple[SourceItem, ...]], ...]
    problems: tuple[ValidationProblem, ...]

    def items(self) -> dict[str, tuple[SourceItem, ...]]:
        """Return inspected source items keyed by declared path."""
        return dict(self.items_by_path)


def expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    """Return a YAML mapping or raise a context-rich error."""
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return {str(key): item for key, item in value.items()}


def expect_list(value: YamlValue, context: str) -> list[YamlValue]:
    """Return a YAML list or raise a context-rich TypeError."""
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def expect_str(value: YamlValue, context: str) -> str:
    """Return a YAML string or raise a context-rich TypeError."""
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    return value


def expect_int(value: YamlValue, context: str) -> int:
    """Return a YAML integer or raise a context-rich TypeError."""
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{context} must be an integer")
    return value


def expect_exact_keys(
    mapping: dict[str, YamlValue],
    expected: set[str],
    context: str,
) -> None:
    """Require an exact mapping key set."""
    if set(mapping) != expected:
        raise ValueError(
            f"{context} keys must be {sorted(expected)}, found {sorted(mapping)}"
        )


def decode_ledger(document: YamlValue, context: str) -> Ledger:
    """Decode and narrow the OPEN-item ledger YAML."""
    root = expect_mapping(document, context)
    expect_exact_keys(root, LEDGER_KEYS, context)

    version = expect_str(root.get("version"), f"{context}.version")
    if version != SUPPORTED_VERSION:
        raise ValueError(
            f"{context}.version must be {SUPPORTED_VERSION!r}, found {version!r}"
        )
    release_history_root = expect_str(
        root.get("release_history_root"),
        f"{context}.release_history_root",
    )
    validate_history_root(
        release_history_root,
        f"{context}.release_history_root",
    )
    release = expect_str(root.get("release"), f"{context}.release")
    parse_release_label(release, f"{context}.release")
    status = expect_str(root.get("status"), f"{context}.status")
    if status not in ALLOWED_STATUSES:
        raise ValueError(
            f"{context}.status must be one of {sorted(ALLOWED_STATUSES)}, "
            f"found {status!r}"
        )
    purpose = expect_str(root.get("purpose"), f"{context}.purpose")

    classification_keys = tuple(
        expect_str(item, f"{context}: classification_keys[{index}]")
        for index, item in enumerate(
            expect_list(
                root.get("classification_keys"),
                f"{context}.classification_keys",
            )
        )
    )

    sources: list[SourceConfig] = []
    for index, raw_source in enumerate(
        expect_list(root.get("sources"), f"{context}.sources")
    ):
        item_context = f"{context}.sources[{index}]"
        source = expect_mapping(raw_source, item_context)
        expect_exact_keys(source, SOURCE_KEYS, item_context)
        sources.append(
            SourceConfig(
                path=expect_str(source.get("path"), f"{item_context}.path"),
                item_count=expect_int(
                    source.get("item_count"),
                    f"{item_context}.item_count",
                ),
            )
        )

    items: list[LedgerItem] = []
    for index, raw_item in enumerate(
        expect_list(root.get("items"), f"{context}.items")
    ):
        item_context = f"{context}.items[{index}]"
        item = expect_mapping(raw_item, item_context)
        expect_exact_keys(item, ITEM_KEYS, item_context)
        items.append(
            LedgerItem(
                item_id=expect_str(item.get("id"), f"{item_context}.id"),
                source_path=expect_str(
                    item.get("source_path"),
                    f"{item_context}.source_path",
                ),
                classification=expect_str(
                    item.get("classification"),
                    f"{item_context}.classification",
                ),
            )
        )

    raw_summary = expect_mapping(root.get("summary"), f"{context}.summary")
    expect_exact_keys(raw_summary, SUMMARY_KEYS, f"{context}.summary")
    summary = {
        key: expect_int(raw_summary.get(key), f"{context}.summary.{key}")
        for key in sorted(SUMMARY_KEYS)
    }

    return Ledger(
        version=version,
        release_history_root=release_history_root,
        release=release,
        status=status,
        purpose=purpose,
        classification_keys=classification_keys,
        sources=tuple(sources),
        items=tuple(items),
        summary=summary,
    )


def resolve_source_path(repo_root: Path, rel_path: str) -> Path:
    """Resolve one canonical POSIX source path beneath the repository root."""
    pure_path = PurePosixPath(rel_path)
    if (
        rel_path == ""
        or "\\" in rel_path
        or pure_path.is_absolute()
        or ".." in pure_path.parts
        or pure_path.as_posix() != rel_path
    ):
        raise ValueError(
            f"OPEN source path must be canonical, relative, and non-traversing: "
            f"{rel_path!r}"
        )

    resolved_root = repo_root.resolve()
    source_path = resolved_root.joinpath(*pure_path.parts)
    try:
        resolved_source = source_path.resolve()
    except RuntimeError as error:
        raise ValueError(
            f"OPEN source path cannot be resolved: {rel_path!r}"
        ) from error
    try:
        resolved_source.relative_to(resolved_root)
    except ValueError as error:
        raise ValueError(
            f"OPEN source path escapes repository root: {rel_path!r}"
        ) from error
    return source_path


def extract_source_items(
    rel_path: str,
    visible_lines: Iterable[tuple[int, str]],
) -> tuple[SourceItem, ...]:
    """Extract stable-ID OPEN items from visible top-level Markdown lines."""
    items: list[SourceItem] = []
    for line_number, line in visible_lines:
        matched = RE_TOP_LEVEL_OPEN_ITEM.match(line)
        if matched is None:
            if RE_TOP_LEVEL_NUMBERED_ITEM.match(line):
                raise ValueError(
                    f"{rel_path}:{line_number}: top-level OPEN item must use "
                    "'N. **`STABLE-ID`** - title'"
                )
            continue
        title = matched.group(3).strip()
        if not title:
            raise ValueError(
                f"{rel_path}:{line_number}: top-level OPEN item title must not be empty"
            )
        items.append(
            SourceItem(
                path=rel_path,
                number=int(matched.group(1)),
                item_id=matched.group(2),
                title=title,
            )
        )
    return tuple(items)


def inspect_sources(
    repo_root: Path,
    sources: tuple[SourceConfig, ...],
) -> SourceInspection:
    """Load declared source documents at the repository boundary."""
    items_by_path: dict[str, tuple[SourceItem, ...]] = {}
    problems: list[ValidationProblem] = []
    for source in sources:
        try:
            source_path = resolve_source_path(repo_root, source.path)
        except ValueError as error:
            problems.append(ValidationProblem(str(error)))
            continue
        if not source_path.is_file():
            problems.append(ValidationProblem(f"source file is missing: {source.path}"))
            continue
        items_by_path[source.path] = extract_source_items(
            source.path,
            visible_markdown_lines(source_path),
        )
    return SourceInspection(
        items_by_path=tuple(sorted(items_by_path.items())),
        problems=tuple(problems),
    )


def reference_counts(items: tuple[LedgerItem, ...]) -> Counter[tuple[str, str]]:
    """Return ledger reference counts keyed by source path and stable ID."""
    return Counter((item.source_path, item.item_id) for item in items)


def validate_ledger(
    ledger: Ledger,
    source_inspection: SourceInspection,
) -> list[ValidationProblem]:
    """Validate a ledger against canonical inspected source items."""
    problems = list(source_inspection.problems)
    declared_source_paths = [source.path for source in ledger.sources]
    source_path_counts = Counter(declared_source_paths)
    for path, count in sorted(source_path_counts.items()):
        if count > 1:
            problems.append(ValidationProblem(f"duplicate source entry: {path}"))

    source_items = source_inspection.items()
    for source in ledger.sources:
        if source.path not in source_items:
            continue
        extracted = source_items[source.path]
        extracted_numbers = [item.number for item in extracted]
        expected_numbers = list(range(1, len(extracted) + 1))
        if extracted_numbers != expected_numbers:
            problems.append(
                ValidationProblem(
                    f"{source.path} top-level OPEN items are not consecutively "
                    f"numbered: {extracted_numbers}"
                )
            )
        if source.item_count != len(extracted):
            problems.append(
                ValidationProblem(
                    f"{source.path} declares item_count {source.item_count}, "
                    f"but document has {len(extracted)} top-level items"
                )
            )

    valid_classifications = set(ledger.classification_keys)
    expected_classifications = {"baseline_must_close", "deferred_nonbaseline"}
    if valid_classifications != expected_classifications:
        problems.append(
            ValidationProblem(
                "classification_keys must be exactly "
                f"{sorted(expected_classifications)}"
            )
        )

    item_id_counts = Counter(item.item_id for item in ledger.items)
    for item_id, count in sorted(item_id_counts.items()):
        if count > 1:
            problems.append(ValidationProblem(f"duplicate ledger item id: {item_id}"))

    for item in ledger.items:
        if RE_OPEN_ITEM_ID.fullmatch(item.item_id) is None:
            problems.append(
                ValidationProblem(f"malformed ledger item id: {item.item_id!r}")
            )
        if item.source_path not in source_items:
            problems.append(
                ValidationProblem(
                    f"{item.item_id} references undeclared source {item.source_path}"
                )
            )
        if item.classification not in valid_classifications:
            problems.append(
                ValidationProblem(
                    f"{item.item_id} has unknown classification {item.classification}"
                )
            )

    expected_refs = {
        (item.path, item.item_id) for items in source_items.values() for item in items
    }
    source_id_counts = Counter(
        item.item_id for items in source_items.values() for item in items
    )
    for item_id, count in sorted(source_id_counts.items()):
        if count > 1:
            problems.append(
                ValidationProblem(
                    f"source OPEN item id {item_id} appears {count} times"
                )
            )

    ledger_ref_counts = reference_counts(ledger.items)
    ledger_refs = set(ledger_ref_counts)
    for ref in sorted(expected_refs - ledger_refs):
        problems.append(
            ValidationProblem(f"missing ledger item for {ref[0]} id {ref[1]}")
        )
    for ref in sorted(ledger_refs - expected_refs):
        problems.append(
            ValidationProblem(f"extra ledger item for {ref[0]} id {ref[1]}")
        )
    for ref, count in sorted(ledger_ref_counts.items()):
        if count > 1:
            problems.append(
                ValidationProblem(
                    f"ledger item for {ref[0]} id {ref[1]} appears {count} times"
                )
            )

    classification_counts = Counter(item.classification for item in ledger.items)
    unclassified_count = sum(
        count
        for classification, count in classification_counts.items()
        if classification not in valid_classifications
    )
    expected_summary = {
        "total_items": len(ledger.items),
        "baseline_must_close_count": classification_counts["baseline_must_close"],
        "deferred_nonbaseline_count": classification_counts["deferred_nonbaseline"],
        "unclassified_count": unclassified_count,
        # The v0.1 ledger has no per-item closed-state field; baseline-close
        # classifications are therefore the open baseline-close count.
        "baseline_must_close_open_count": classification_counts["baseline_must_close"],
    }
    for key, expected in sorted(expected_summary.items()):
        actual = ledger.summary[key]
        if actual != expected:
            problems.append(
                ValidationProblem(f"summary.{key} is {actual}, expected {expected}")
            )

    return problems

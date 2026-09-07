"""Canonical authoritative-artifact inventory and repository validation."""

import stat
import subprocess
from collections import defaultdict
from collections.abc import Set as AbstractSet
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from eglur_conformance.release_identity import (
    parse_release_label,
    validate_history_root,
)
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue

DEFAULT_INVENTORY_PATH = Path("docs/spec/artifact-inventory-v0.1.yaml")
SUPPORTED_VERSION = "0.1"
ALLOWED_STATUSES = frozenset({"draft", "released"})
INVENTORY_KEYS = frozenset(
    {
        "version",
        "release_history_root",
        "release",
        "status",
        "artifacts",
    }
)
ARTIFACT_KEYS = frozenset({"selector", "role", "provenance", "status_source"})
ALLOWED_CLASSIFICATIONS = frozenset(
    {
        ("normative_spec", "authored", "inventory"),
        ("release_control", "authored", "inventory"),
        ("release_control", "authored", "self"),
        ("conformance_definition", "authored", "inventory"),
        ("conformance_definition", "authored", "self"),
        ("conformance_definition", "authored", "manifest"),
        ("generated_evidence", "generated", "self"),
    }
)
AUTHORITATIVE_ROOTS = ("docs/spec/", "tests/conformance/")
AUTHORITATIVE_SUFFIXES = frozenset({".md", ".yaml", ".yml"})
REGULAR_GIT_MODES = frozenset({"100644", "100755"})
SELF_STATUS_VALIDATOR_PATHS = frozenset(
    {
        "docs/spec/artifact-inventory-v0.1.yaml",
        "docs/spec/open-items-ledger-v0.1.yaml",
        "tests/conformance/manifest.yaml",
        "tests/conformance/traceability.yaml",
    }
)
GENERATED_PARITY_VALIDATOR_PATHS = frozenset({"tests/conformance/traceability.yaml"})
MANIFEST_STATUS_PREFIXES = (
    "tests/conformance/core/",
    "tests/conformance/optional/",
    "tests/conformance/tensor/",
)
_REGISTERED_IMPLEMENTATION_PROBES = frozenset(
    {"tests/conformance/probes/registry_cache.yaml"}
)


@dataclass(frozen=True)
class ArtifactRule:
    """One path selector and its orthogonal artifact classification."""

    selector: str
    role: str
    provenance: str
    status_source: str

    @property
    def classification(self) -> tuple[str, str, str]:
        """Return the rule's role, provenance, and status source."""
        return self.role, self.provenance, self.status_source


@dataclass(frozen=True)
class ArtifactInventory:
    """Closed v0.1 inventory of authoritative repository artifacts."""

    version: str
    release_history_root: str
    release: str
    status: str
    artifacts: tuple[ArtifactRule, ...]


@dataclass(frozen=True)
class TrackedArtifact:
    """Git index identity for one authoritative artifact."""

    mode: str
    object_id: str


def expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    """Return a YAML mapping or raise a context-rich error."""
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return {str(key): item for key, item in value.items()}


def expect_list(value: YamlValue, context: str) -> list[YamlValue]:
    """Return a YAML list or raise a context-rich error."""
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def expect_str(value: YamlValue, context: str) -> str:
    """Return a non-empty YAML string."""
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if not value:
        raise ValueError(f"{context} must be non-empty")
    return value


def expect_exact_keys(
    value: dict[str, YamlValue],
    expected: AbstractSet[str],
    context: str,
) -> None:
    """Reject missing and unsupported mapping keys."""
    missing = sorted(expected - set(value))
    unknown = sorted(set(value) - expected)
    if missing or unknown:
        details: list[str] = []
        if missing:
            details.append(f"missing {missing}")
        if unknown:
            details.append(f"unsupported {unknown}")
        raise ValueError(f"{context} has invalid keys: {', '.join(details)}")


def decode_inventory(
    document: YamlValue,
    context: str,
) -> ArtifactInventory:
    """Decode the artifact inventory under its closed schema."""
    root = expect_mapping(document, context)
    expect_exact_keys(root, INVENTORY_KEYS, context)

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
            f"{context}.status must be one of "
            f"{sorted(ALLOWED_STATUSES)}, found {status!r}"
        )

    artifacts: list[ArtifactRule] = []
    for index, raw_artifact in enumerate(
        expect_list(root.get("artifacts"), f"{context}.artifacts")
    ):
        item_context = f"{context}.artifacts[{index}]"
        artifact = expect_mapping(raw_artifact, item_context)
        expect_exact_keys(artifact, ARTIFACT_KEYS, item_context)
        rule = ArtifactRule(
            selector=expect_str(
                artifact.get("selector"),
                f"{item_context}.selector",
            ),
            role=expect_str(artifact.get("role"), f"{item_context}.role"),
            provenance=expect_str(
                artifact.get("provenance"),
                f"{item_context}.provenance",
            ),
            status_source=expect_str(
                artifact.get("status_source"),
                f"{item_context}.status_source",
            ),
        )
        if rule.classification not in ALLOWED_CLASSIFICATIONS:
            raise ValueError(
                f"{item_context} has unsupported classification {rule.classification!r}"
            )
        artifacts.append(rule)

    selectors = [artifact.selector for artifact in artifacts]
    if len(selectors) != len(set(selectors)):
        raise ValueError(f"{context}.artifacts selectors must be unique")

    return ArtifactInventory(
        version=version,
        release_history_root=release_history_root,
        release=release,
        status=status,
        artifacts=tuple(artifacts),
    )


def is_authoritative_artifact_path(path: str) -> bool:
    """Return whether one repository path belongs to the inventory boundary."""
    suffix = PurePosixPath(path).suffix.lower()
    return (path.startswith("docs/spec/") and suffix in AUTHORITATIVE_SUFFIXES) or (
        path.startswith("tests/conformance/") and suffix in {".yaml", ".yml"}
    )


def discover_authoritative_artifacts(repo_root: Path) -> set[str]:
    """Discover every Markdown or YAML artifact in authoritative roots."""
    return {
        path.relative_to(repo_root).as_posix()
        for root in ("docs/spec", "tests/conformance")
        for path in (repo_root / root).rglob("*")
        if (path.is_file() or path.is_symlink())
        and is_authoritative_artifact_path(path.relative_to(repo_root).as_posix())
    }


def tracked_artifacts(repo_root: Path) -> dict[str, TrackedArtifact]:
    """Return index identities for tracked authoritative artifact paths."""
    completed = subprocess.run(
        [
            "git",
            "ls-files",
            "--stage",
            "-z",
            "--",
            "docs/spec",
            "tests/conformance",
        ],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or "git ls-files failed"
        raise ValueError(f"cannot inspect tracked artifact set: {detail}")

    artifacts: dict[str, TrackedArtifact] = {}
    for record in completed.stdout.split("\0"):
        if not record:
            continue
        metadata, path = record.split("\t", 1)
        mode, object_id, stage = metadata.split(" ", 2)
        if stage != "0":
            raise ValueError(
                f"authoritative artifact has unresolved index stage: {path}"
            )
        if is_authoritative_artifact_path(path):
            artifacts[path] = TrackedArtifact(mode=mode, object_id=object_id)
    return artifacts


def raw_worktree_blob_ids(
    repo_root: Path,
    paths: list[str],
) -> dict[str, str]:
    """Hash raw working-tree bytes without clean filters."""
    if not paths:
        return {}

    completed = subprocess.run(
        ["git", "hash-object", "--no-filters", "--", *paths],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or "git hash-object failed"
        raise ValueError(f"cannot hash authoritative artifacts: {detail}")

    object_ids = completed.stdout.splitlines()
    if len(object_ids) != len(paths):
        raise ValueError(
            "git hash-object returned an unexpected authoritative artifact count"
        )
    return dict(zip(paths, object_ids, strict=True))


def validate_checkout_binding(repo_root: Path) -> list[ValidationProblem]:
    """Bind authoritative working-tree files to regular Git index blobs."""
    candidates = discover_authoritative_artifacts(repo_root)
    indexed = tracked_artifacts(repo_root)
    tracked = set(indexed)
    problems: list[ValidationProblem] = []
    hashable_paths: list[str] = []

    for path in sorted(candidates - tracked):
        problems.append(
            ValidationProblem(
                f"authoritative artifact is not tracked in the Git index: {path}"
            )
        )
    for path in sorted(tracked - candidates):
        problems.append(
            ValidationProblem(
                f"tracked authoritative artifact is missing from the checkout: {path}"
            )
        )

    checked_ancestors: set[str] = set()
    for path in sorted(tracked):
        for ancestor in PurePosixPath(path).parents:
            ancestor_path = ancestor.as_posix()
            if ancestor_path == "." or ancestor_path in checked_ancestors:
                continue
            checked_ancestors.add(ancestor_path)
            try:
                ancestor_mode = (repo_root / ancestor_path).lstat().st_mode
            except FileNotFoundError:
                continue
            if stat.S_ISLNK(ancestor_mode):
                problems.append(
                    ValidationProblem(
                        f"authoritative artifact ancestor {ancestor_path} "
                        "must not be a symlink"
                    )
                )

    for path, artifact in sorted(indexed.items()):
        if artifact.mode not in REGULAR_GIT_MODES:
            problems.append(
                ValidationProblem(
                    f"authoritative artifact {path} must be a regular tracked file, "
                    f"found Git mode {artifact.mode}"
                )
            )
            continue

        worktree_path = repo_root / path
        try:
            worktree_mode = worktree_path.lstat().st_mode
        except FileNotFoundError:
            continue
        if not stat.S_ISREG(worktree_mode):
            problems.append(
                ValidationProblem(
                    f"working-tree authoritative artifact {path} must be a regular file"
                )
            )
            continue

        hashable_paths.append(path)

    worktree_object_ids = raw_worktree_blob_ids(repo_root, hashable_paths)
    for path, actual_object_id in worktree_object_ids.items():
        artifact = indexed[path]
        if actual_object_id != artifact.object_id:
            problems.append(
                ValidationProblem(
                    f"working-tree bytes for authoritative artifact {path} "
                    "do not match the Git index"
                )
            )

    for path in sorted(candidates):
        suffix = PurePosixPath(path).suffix
        if suffix.lower() in {".yaml", ".yml"} and suffix != ".yaml":
            problems.append(
                ValidationProblem(
                    f"authoritative YAML must use the exact .yaml extension: {path}"
                )
            )

    return problems


def validate_selector(selector: str) -> str | None:
    """Return a selector error or None when it stays in authoritative roots."""
    pure = PurePosixPath(selector)
    if pure.is_absolute() or ".." in pure.parts:
        return "must be a relative non-traversing repository pattern"
    if not selector.startswith(AUTHORITATIVE_ROOTS):
        return f"must start with one of {list(AUTHORITATIVE_ROOTS)}"
    if not selector.endswith((".md", ".yaml")):
        return "must select Markdown or YAML artifacts"
    return None


def expected_classification(path: str) -> tuple[str, str, str] | None:
    """Return the required classification for established v0.1 artifact families."""
    if path.endswith(".md") and path.startswith("docs/spec/open-items/"):
        return "release_control", "authored", "inventory"
    if path.endswith(".md") and path.startswith("docs/spec/"):
        return "normative_spec", "authored", "inventory"
    if path in {
        "docs/spec/errors/diagnostic-contracts.yaml",
        "docs/spec/stdlib/effect-summaries.yaml",
    }:
        return "normative_spec", "authored", "inventory"
    if path in {
        "docs/spec/artifact-inventory-v0.1.yaml",
        "docs/spec/open-items-ledger-v0.1.yaml",
    }:
        return "release_control", "authored", "self"
    if path == "tests/conformance/manifest.yaml":
        return "conformance_definition", "authored", "self"
    if path == "tests/conformance/traceability.yaml":
        return "generated_evidence", "generated", "self"
    if path == "tests/conformance/probes/registry_cache.yaml":
        return "conformance_definition", "authored", "inventory"
    if path.endswith(".yaml") and path.startswith(
        (
            "tests/conformance/core/",
            "tests/conformance/optional/",
            "tests/conformance/tensor/",
        )
    ):
        return "conformance_definition", "authored", "manifest"
    return None


def validate_inventory(
    inventory: ArtifactInventory,
    candidates: frozenset[str] | set[str],
) -> list[ValidationProblem]:
    """Validate complete, unique, and semantically accurate classification."""
    rules_by_path: dict[str, list[ArtifactRule]] = defaultdict(list)
    problems: list[ValidationProblem] = []

    for rule in inventory.artifacts:
        selector_problem = validate_selector(rule.selector)
        if selector_problem is not None:
            problems.append(
                ValidationProblem(
                    f"artifact selector {rule.selector!r} {selector_problem}"
                )
            )
            continue

        matches = sorted(
            path for path in candidates if PurePosixPath(path).match(rule.selector)
        )
        if not matches:
            problems.append(
                ValidationProblem(
                    f"artifact selector {rule.selector!r} matches no files"
                )
            )
            continue

        for path in matches:
            rules_by_path[path].append(rule)

    for path in sorted(candidates):
        suffix = PurePosixPath(path).suffix
        if suffix.lower() in {".yaml", ".yml"} and suffix != ".yaml":
            continue

        if (
            path.startswith("tests/conformance/probes/")
            and path not in _REGISTERED_IMPLEMENTATION_PROBES
        ):
            problems.append(
                ValidationProblem(f"unregistered implementation probe: {path}")
            )
            continue

        rules = rules_by_path[path]
        if not rules:
            problems.append(ValidationProblem(f"unclassified artifact: {path}"))
            continue
        if len(rules) > 1:
            selectors = [rule.selector for rule in rules]
            problems.append(
                ValidationProblem(
                    f"artifact {path} is classified by multiple selectors: {selectors}"
                )
            )
            continue

        expected = expected_classification(path)
        if expected is not None and rules[0].classification != expected:
            problems.append(
                ValidationProblem(
                    f"artifact {path} classification is "
                    f"{rules[0].classification!r}, expected {expected!r}"
                )
            )

        rule = rules[0]
        if path in _REGISTERED_IMPLEMENTATION_PROBES and rule.selector != path:
            problems.append(
                ValidationProblem(
                    f"registered implementation probe {path} must use an exact selector"
                )
            )
        if path.startswith("docs/spec/") and rule.role not in {
            "normative_spec",
            "release_control",
        }:
            problems.append(
                ValidationProblem(
                    f"artifact {path} uses role {rule.role!r} outside its root"
                )
            )
        if path.startswith("tests/conformance/") and rule.role not in {
            "conformance_definition",
            "generated_evidence",
        }:
            problems.append(
                ValidationProblem(
                    f"artifact {path} uses role {rule.role!r} outside its root"
                )
            )
        if rule.status_source == "self" and path not in SELF_STATUS_VALIDATOR_PATHS:
            problems.append(
                ValidationProblem(
                    f"self-status artifact {path} has no registered status validator"
                )
            )
        if rule.status_source == "manifest" and not path.startswith(
            MANIFEST_STATUS_PREFIXES
        ):
            problems.append(
                ValidationProblem(
                    f"manifest-status artifact {path} is outside suite roots"
                )
            )
        if (
            rule.provenance == "generated"
            and path not in GENERATED_PARITY_VALIDATOR_PATHS
        ):
            problems.append(
                ValidationProblem(
                    f"generated artifact {path} has no registered parity validator"
                )
            )

    return problems

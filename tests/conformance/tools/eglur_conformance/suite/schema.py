"""Validate manifest and conformance test-case schema invariants."""

import re
from collections import Counter
from pathlib import Path
from typing import TypeAlias, cast

from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.diagnostics.registry import load_diagnostic_registry
from eglur_conformance.preconditions import V01_PRECONDITIONS
from eglur_conformance.release_identity import (
    parse_release_label,
    validate_history_root,
)
from eglur_conformance.suite import model
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue, load_yaml

DEFAULT_MANIFEST_PATH = Path("tests/conformance/manifest.yaml")
EXCLUDED_SUITE_FILES = {"manifest.yaml", "traceability.yaml"}
SUITE_DIRECTORIES = ("core", "optional", "tensor")
SUPPORTED_VERSION = "0.1"
CONFORMANCE_ROOT = "tests/conformance"
ALLOWED_ARTIFACT_STATUSES = {"draft", "released"}
MANIFEST_KEYS = {
    "version",
    "release_history_root",
    "release",
    "status",
    "root",
    "suites",
}
MANIFEST_SUITE_REQUIRED_KEYS = {"id", "file", "required"}
MANIFEST_SUITE_OPTIONAL_KEYS = {"precondition"}
SUITE_FILE_KEYS = {"tests"}
TEST_CASE_KEYS = {
    "id",
    "category",
    "title",
    "code",
    "code_bytes_hex",
    "files",
    "symlinks",
    "registry_acquisitions",
    "entry",
    "file_set_root",
    "mode",
    "runner",
    "expect",
}
VALUE_ASSERTION_KEYS = {"kind", "repr", "error_variant", "numeric_compare"}
NUMERIC_COMPARE_KEYS = {"dtype", "expected", "shape", "mode", "rtol", "atol"}
NUMERIC_COMPARE_MODES = {"exact", "approx"}
ALLOWED_MODES = {"auto", "package", "file-set"}
ALLOWED_RUNNERS = {"main", "test"}
_REGISTRY_LINK_KINDS = frozenset({"symlink", "hardlink"})
_REGISTRY_SPECIAL_KINDS = frozenset(
    {
        "block_device",
        "character_device",
        "fifo",
        "socket",
        "other_special",
    }
)
_REGISTRY_ENTRY_KINDS = (
    frozenset({"file", "directory"}) | _REGISTRY_LINK_KINDS | _REGISTRY_SPECIAL_KINDS
)
PRIMARY_OUTCOMES = {"compile_error", "runtime_error", "test_run", "value"}
SECONDARY_EXPECT_KEYS = {
    "value_diagnostics",
    "diagnostic_contains",
    "side_effects",
    "process_exit",
}
DIAGNOSTIC_REQUIRED_LIST_FIELDS = {"message_contains", "data_keys"}
DIAGNOSTIC_OPTIONAL_LIST_FIELDS = {"help_contains", "related_contains"}
DIAGNOSTIC_SUPPORTED_FIELDS = (
    {"code", "message_contains_any", "data_equals"}
    | DIAGNOSTIC_REQUIRED_LIST_FIELDS
    | DIAGNOSTIC_OPTIONAL_LIST_FIELDS
)
COMPILE_DIAGNOSTIC_FIELDS = {"primary_span"}
_PRIMARY_MARKER_SPAN_FIELDS = frozenset({"marker"})
_PRIMARY_BYTE_SPAN_FIELDS = frozenset({"source", "bytes"})
_PRIMARY_BYTE_RANGE_FIELDS = frozenset({"start", "end"})
TEST_RUN_FAILURE_KINDS = {"returned_err", "runtime_abort"}
RE_PRIMARY_MARKER_NAME = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*")
RE_PRIMARY_MARKER_COMMENT = re.compile(
    r"//[ \t]*@primary[ \t]+"
    r"(?P<name>[a-z][a-z0-9]*(?:-[a-z0-9]+)*)[ \t]*$"
)
RE_PRIMARY_MARKER_PREFIX = re.compile(r"//[ \t]*@primary(?:[ \t]|$)")
RE_IDENTIFIER_ASSIGNMENT = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)[ \t]*=(?!=|>)")
RE_LET_DECLARATION = re.compile(r"let(?P<mutable>[ \t]+mut)?[ \t]+(?P<body>.+)$")
RE_IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
RE_TYPED_BARE_BINDING = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)[ \t]*:[^=\n]+=(?!=|>)")
_REGISTRY_AUTHORITY_LABEL_PATTERN = re.compile(r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?")
_REGISTRY_PATH_SEGMENT_PATTERN = re.compile(r"[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?")
_EXACT_PACKAGE_VERSION_PATTERN = re.compile(
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
)
INTENTIONAL_BARE_ASSIGNMENTS = {
    "CFL-immutable-let-reassignment-in-statically-false-if-branch-is-still-rejected": (
        "x",
        "IMMUTABLE_BINDING_ASSIGN",
        "IMMUTABLE_BINDING_ASSIGN",
    ),
    "LOP-immutable-let-reassignment-in-zero-iteration-for-body-is-compile-time-error": (
        "x",
        "IMMUTABLE_BINDING_ASSIGN",
        "IMMUTABLE_BINDING_ASSIGN",
    ),
    "TYC-bare-assignment-does-not-introduce-local": (
        "x",
        "ASSIGN_TARGET_NOT_ASSIGNABLE",
        "ASSIGN_TARGET_NOT_ASSIGNABLE",
    ),
    "TYC-immutable-let-binding-rejects-reassignment": (
        "x",
        "IMMUTABLE_BINDING_ASSIGN",
        "IMMUTABLE_BINDING_ASSIGN",
    ),
    "TYC-direct-assignment-to-captured-binding-uses-capture-diagnostic": (
        "captured",
        "MUTABLE_CAPTURE_FORBIDDEN",
        "IMMUTABLE_BINDING_ASSIGN",
    ),
}


DiagnosticRegistry: TypeAlias = diagnostic_model.DiagnosticRegistry


def expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    """Return a YAML mapping or raise a context-rich error."""
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def expect_list(value: YamlValue, context: str) -> list[YamlValue]:
    """Return a YAML list or raise a context-rich TypeError."""
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def expect_str(value: YamlValue, context: str) -> str:
    """Return a non-empty YAML string or raise a context-rich error."""
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if value == "":
        raise ValueError(f"{context} must be non-empty")
    return value


def expect_bool(value: YamlValue, context: str) -> bool:
    """Return a YAML bool or raise a context-rich TypeError."""
    if not isinstance(value, bool):
        raise TypeError(f"{context} must be a boolean")
    return value


def expect_exact_keys(
    mapping: dict[str, YamlValue],
    required: set[str],
    optional: set[str],
    context: str,
) -> None:
    """Require all mandatory keys and reject fields outside a closed schema."""
    missing = sorted(required - set(mapping))
    unknown = sorted(set(mapping) - required - optional)
    if missing or unknown:
        details: list[str] = []
        if missing:
            details.append(f"missing {missing}")
        if unknown:
            details.append(f"unsupported {unknown}")
        raise ValueError(f"{context} has invalid keys: {', '.join(details)}")


def _utf8_byte_length(value: str) -> int | None:
    """Return the UTF-8 length of Unicode-scalar text, or None if malformed."""
    try:
        return len(value.encode("utf-8"))
    except UnicodeEncodeError:
        return None


def is_canonical_relative_posix_path(
    value: str,
    *,
    allow_current_directory: bool = False,
) -> bool:
    """Return whether a path is relative POSIX syntax without normalization."""
    if allow_current_directory and value == ".":
        return True
    is_drive_qualified = (
        len(value) >= 2
        and value[0].isascii()
        and value[0].isalpha()
        and value[1] == ":"
    )
    if (
        value == ""
        or value.startswith("/")
        or is_drive_qualified
        or "\\" in value
        or "\0" in value
        or _utf8_byte_length(value) is None
    ):
        return False
    return all(part not in {"", ".", ".."} for part in value.split("/"))


def _lexical_fixture_path_parts(value: str) -> tuple[str, ...] | None:
    is_drive_qualified = (
        len(value) >= 2
        and value[0].isascii()
        and value[0].isalpha()
        and value[1] == ":"
    )
    if (
        value == ""
        or value.startswith("/")
        or is_drive_qualified
        or "\\" in value
        or "\0" in value
    ):
        return None
    parts: list[str] = []
    for part in value.split("/"):
        if part in {"", "."}:
            continue
        if part == "..":
            return None
        parts.append(part)
    return tuple(parts) or None


def _is_contained_symlink_target(link_path: str, target: str) -> bool:
    is_drive_qualified = (
        len(target) >= 2
        and target[0].isascii()
        and target[0].isalpha()
        and target[1] == ":"
    )
    if (
        target == ""
        or target.startswith("/")
        or is_drive_qualified
        or "\\" in target
        or "\0" in target
    ):
        return False
    target_parts = target.split("/")
    if any(part == "" for part in target_parts):
        return False

    resolved = list(link_path.split("/")[:-1])
    for part in target_parts:
        if part == ".":
            continue
        if part == "..":
            if not resolved:
                return False
            resolved.pop()
        else:
            resolved.append(part)
    return True


def _path_is_ancestor(ancestor: tuple[str, ...], descendant: tuple[str, ...]) -> bool:
    return len(ancestor) < len(descendant) and descendant[: len(ancestor)] == ancestor


def _is_registry_package_id(value: str) -> bool:
    authority, separator, package_path = value.partition("/")
    if separator == "" or package_path == "":
        return False
    authority_labels = authority.split(".")
    if len(authority_labels) < 2 or any(
        len(label) > 63 or _REGISTRY_AUTHORITY_LABEL_PATTERN.fullmatch(label) is None
        for label in authority_labels
    ):
        return False
    return all(
        _REGISTRY_PATH_SEGMENT_PATTERN.fullmatch(segment) is not None
        for segment in package_path.split("/")
    )


def _validate_registry_byte_form(
    value: dict[str, YamlValue],
    text_key: str,
    hex_key: str,
    repeat_key: str,
    context: str,
) -> list[ValidationProblem]:
    present = [key for key in (text_key, hex_key, repeat_key) if key in value]
    if len(present) != 1:
        return [
            ValidationProblem(
                f"{context} must provide exactly one of "
                f"{text_key}, {hex_key}, or {repeat_key}"
            )
        ]

    selected = present[0]
    raw_value = value[selected]
    if selected == repeat_key:
        if not isinstance(raw_value, dict) or not all(
            isinstance(key, str) for key in raw_value
        ):
            return [ValidationProblem(f"{context}.{repeat_key} must be a mapping")]
        repeated = cast(dict[str, YamlValue], raw_value)
        if set(repeated) != {"byte_hex", "count"}:
            return [
                ValidationProblem(
                    f"{context}.{repeat_key} must contain exactly byte_hex and count"
                )
            ]
        byte_hex = repeated["byte_hex"]
        count = repeated["count"]
        problems: list[ValidationProblem] = []
        if (
            not isinstance(byte_hex, str)
            or len(byte_hex) != 2
            or any(character not in "0123456789abcdef" for character in byte_hex)
        ):
            problems.append(
                ValidationProblem(
                    f"{context}.{repeat_key}.byte_hex must be exactly one "
                    "canonical lowercase hex byte"
                )
            )
        if (
            type(count) is not int
            or count <= 0
            or count > model.REGISTRY_PROVIDER_REPEAT_COUNT_MAX
        ):
            problems.append(
                ValidationProblem(
                    f"{context}.{repeat_key}.count must be a positive i64"
                )
            )
        return problems

    if not isinstance(raw_value, str):
        return [ValidationProblem(f"{context}.{selected} must be a string")]
    if selected == text_key:
        try:
            raw_value.encode("utf-8")
        except UnicodeEncodeError:
            return [
                ValidationProblem(f"{context}.{text_key} must be Unicode scalar text")
            ]
        return []
    if len(raw_value) % 2 != 0 or any(
        character not in "0123456789abcdef" for character in raw_value
    ):
        return [
            ValidationProblem(
                f"{context}.{hex_key} must be canonical lowercase even-length hex"
            )
        ]
    return []


def _validate_registry_entry(
    value: YamlValue,
    context: str,
) -> list[ValidationProblem]:
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        return [ValidationProblem(f"{context} must be a string-keyed mapping")]

    entry = cast(dict[str, YamlValue], value)
    kind = entry.get("kind")
    if not isinstance(kind, str) or kind not in _REGISTRY_ENTRY_KINDS:
        return [
            ValidationProblem(
                f"{context}.kind must be one of {sorted(_REGISTRY_ENTRY_KINDS)}"
            )
        ]

    allowed = {"kind", "path", "path_hex", "path_repeat"}
    if kind == "file":
        allowed |= {"text", "content_hex", "content_repeat"}
    elif kind in _REGISTRY_LINK_KINDS:
        allowed |= {"target", "target_hex", "target_repeat"}
    unknown = set(entry) - allowed
    problems: list[ValidationProblem] = []
    if unknown:
        problems.append(
            ValidationProblem(f"{context} has unsupported keys: {sorted(unknown)}")
        )
    problems.extend(
        _validate_registry_byte_form(
            entry,
            "path",
            "path_hex",
            "path_repeat",
            context,
        )
    )
    if kind == "file":
        problems.extend(
            _validate_registry_byte_form(
                entry,
                "text",
                "content_hex",
                "content_repeat",
                context,
            )
        )
    elif kind in _REGISTRY_LINK_KINDS:
        problems.extend(
            _validate_registry_byte_form(
                entry,
                "target",
                "target_hex",
                "target_repeat",
                context,
            )
        )
    return problems


def _validate_registry_provider_step(
    value: YamlValue,
    context: str,
) -> list[ValidationProblem]:
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        return [ValidationProblem(f"{context} must be a string-keyed mapping")]
    step = cast(dict[str, YamlValue], value)
    if len(step) != 1 or not set(step) <= {"emit", "repeat", "finish"}:
        return [
            ValidationProblem(
                f"{context} must contain exactly one of emit, repeat, or finish"
            )
        ]
    if "emit" in step:
        return _validate_registry_entry(step["emit"], f"{context}.emit")
    if "finish" in step:
        finish = step["finish"]
        if (
            not isinstance(finish, str)
            or finish not in model.REGISTRY_PROVIDER_FINISHES
        ):
            return [
                ValidationProblem(
                    f"{context}.finish must be one of "
                    f"{sorted(model.REGISTRY_PROVIDER_FINISHES)}"
                )
            ]
        return []

    raw_repeat = step["repeat"]
    if not isinstance(raw_repeat, dict) or not all(
        isinstance(key, str) for key in raw_repeat
    ):
        return [ValidationProblem(f"{context}.repeat must be a mapping")]
    repeated = cast(dict[str, YamlValue], raw_repeat)
    if set(repeated) != {"count", "emit"}:
        return [
            ValidationProblem(f"{context}.repeat must contain exactly count and emit")
        ]
    problems: list[ValidationProblem] = []
    count = repeated["count"]
    if (
        type(count) is not int
        or count <= 0
        or count > model.REGISTRY_PROVIDER_REPEAT_COUNT_MAX
    ):
        problems.append(
            ValidationProblem(f"{context}.repeat.count must be a positive i64")
        )
    problems.extend(
        _validate_registry_entry(repeated["emit"], f"{context}.repeat.emit")
    )
    return problems


def _validate_registry_acquisitions(
    value: YamlValue,
    context: str,
) -> list[ValidationProblem]:
    if not isinstance(value, list):
        return [ValidationProblem(f"{context} must be a list")]
    if not value:
        return [ValidationProblem(f"{context} must be non-empty")]

    problems: list[ValidationProblem] = []
    coordinates: set[tuple[str, str]] = set()
    for index, raw_acquisition in enumerate(value):
        acquisition_context = f"{context}[{index}]"
        if not isinstance(raw_acquisition, dict) or not all(
            isinstance(key, str) for key in raw_acquisition
        ):
            problems.append(
                ValidationProblem(f"{acquisition_context} must be a mapping")
            )
            continue
        acquisition = cast(dict[str, YamlValue], raw_acquisition)
        required = {"package", "version", "provider_steps"}
        unknown = set(acquisition) - required
        missing = required - set(acquisition)
        if missing or unknown:
            problems.append(
                ValidationProblem(
                    f"{acquisition_context} has invalid keys: "
                    f"missing={sorted(missing)}, unsupported={sorted(unknown)}"
                )
            )

        package = acquisition.get("package")
        version = acquisition.get("version")
        if not isinstance(package, str) or not _is_registry_package_id(package):
            problems.append(
                ValidationProblem(
                    f"{acquisition_context}.package must be a canonical "
                    "RegistryPackageId"
                )
            )
        if (
            not isinstance(version, str)
            or _EXACT_PACKAGE_VERSION_PATTERN.fullmatch(version) is None
        ):
            problems.append(
                ValidationProblem(
                    f"{acquisition_context}.version must be an exact version"
                )
            )
        if isinstance(package, str) and isinstance(version, str):
            coordinate = (package, version)
            if coordinate in coordinates:
                problems.append(
                    ValidationProblem(
                        f"{acquisition_context} duplicates registry coordinate "
                        f"{coordinate}"
                    )
                )
            coordinates.add(coordinate)

        provider_steps = acquisition.get("provider_steps")
        if not isinstance(provider_steps, list) or not provider_steps:
            problems.append(
                ValidationProblem(
                    f"{acquisition_context}.provider_steps must be a non-empty list"
                )
            )
            continue

        for step_index, step in enumerate(provider_steps):
            step_context = f"{acquisition_context}.provider_steps[{step_index}]"
            problems.extend(_validate_registry_provider_step(step, step_context))
            is_finish = isinstance(step, dict) and set(step) == {"finish"}
            if step_index == len(provider_steps) - 1:
                if not is_finish:
                    problems.append(
                        ValidationProblem(
                            f"{step_context} must be the final finish step"
                        )
                    )
            elif is_finish:
                problems.append(
                    ValidationProblem(
                        f"{step_context} cannot finish before the final step"
                    )
                )

        final_step = provider_steps[-1]
        if (
            isinstance(final_step, dict)
            and final_step.get("finish") == "missing_release"
            and len(provider_steps) != 1
        ):
            problems.append(
                ValidationProblem(
                    f"{acquisition_context}.provider_steps must contain only "
                    "finish: missing_release for a missing release"
                )
            )
    return problems


def _validate_symlink_topology(
    raw_symlinks: YamlValue,
    files: dict[str, YamlValue],
    context: str,
) -> list[ValidationProblem]:
    problems: list[ValidationProblem] = []
    if not isinstance(raw_symlinks, dict):
        return [ValidationProblem(f"{context}.symlinks must be a mapping")]
    if not raw_symlinks:
        return [ValidationProblem(f"{context}.symlinks must be non-empty")]
    if not all(isinstance(key, str) for key in raw_symlinks):
        return [ValidationProblem(f"{context}.symlinks must use string keys")]

    symlinks = cast(dict[str, YamlValue], raw_symlinks)
    valid_links: dict[str, tuple[str, ...]] = {}
    for link_path, raw_target in symlinks.items():
        link_context = f"{context}.symlinks.{link_path}"
        if not is_canonical_relative_posix_path(link_path):
            problems.append(
                ValidationProblem(f"{context}.symlinks has invalid path {link_path}")
            )
            continue
        valid_links[link_path] = tuple(link_path.split("/"))
        if not isinstance(raw_target, str):
            problems.append(ValidationProblem(f"{link_context} must be a string"))
        elif not _is_contained_symlink_target(link_path, raw_target):
            problems.append(
                ValidationProblem(
                    f"{link_context} must be a non-empty relative POSIX target "
                    "contained in the fixture root"
                )
            )

    regular_paths = {
        path: parts
        for path in files
        if (parts := _lexical_fixture_path_parts(path)) is not None
    }
    for link_path, link_parts in valid_links.items():
        for file_path, file_parts in regular_paths.items():
            if link_parts == file_parts:
                problems.append(
                    ValidationProblem(
                        f"{context}.symlinks path {link_path} conflicts with "
                        f"regular file {file_path}"
                    )
                )
            elif _path_is_ancestor(file_parts, link_parts):
                problems.append(
                    ValidationProblem(
                        f"{context}.symlinks path {link_path} is below regular "
                        f"file {file_path}"
                    )
                )
            elif _path_is_ancestor(link_parts, file_parts):
                problems.append(
                    ValidationProblem(
                        f"{context}.files path {file_path} is below symlink {link_path}"
                    )
                )

        for other_path, other_parts in valid_links.items():
            if link_path >= other_path:
                continue
            if _path_is_ancestor(link_parts, other_parts) or _path_is_ancestor(
                other_parts, link_parts
            ):
                problems.append(
                    ValidationProblem(
                        f"{context}.symlinks paths {link_path} and {other_path} "
                        "must not be nested"
                    )
                )
    return problems


def is_scalar(value: YamlValue) -> bool:
    """Return True when a YAML value is scalar under the conformance contract."""
    return value is None or isinstance(value, str | int | float | bool)


def parse_manifest_value(
    value: YamlValue,
    context: str = "manifest",
) -> model.Manifest:
    """Parse and narrow one conformance manifest value."""
    root = expect_mapping(value, context)
    expect_exact_keys(root, MANIFEST_KEYS, set(), "manifest")
    version = expect_str(root.get("version"), "manifest.version")
    if version != SUPPORTED_VERSION:
        raise ValueError(
            f"manifest.version must be {SUPPORTED_VERSION!r}, found {version!r}"
        )
    release_history_root = expect_str(
        root.get("release_history_root"),
        "manifest.release_history_root",
    )
    validate_history_root(
        release_history_root,
        "manifest.release_history_root",
    )
    release = expect_str(root.get("release"), "manifest.release")
    parse_release_label(release, "manifest.release")
    status = expect_str(root.get("status"), "manifest.status")
    if status not in ALLOWED_ARTIFACT_STATUSES:
        raise ValueError(
            "manifest.status must be one of "
            f"{sorted(ALLOWED_ARTIFACT_STATUSES)}, found {status!r}"
        )
    suite_root = expect_str(root.get("root"), "manifest.root")
    if suite_root != CONFORMANCE_ROOT:
        raise ValueError(
            f"manifest.root must be {CONFORMANCE_ROOT!r}, found {suite_root!r}"
        )

    suites: list[model.SuiteEntry] = []
    for index, raw_suite in enumerate(
        expect_list(root.get("suites"), "manifest.suites")
    ):
        suite = expect_mapping(raw_suite, f"manifest.suites[{index}]")
        expect_exact_keys(
            suite,
            MANIFEST_SUITE_REQUIRED_KEYS,
            MANIFEST_SUITE_OPTIONAL_KEYS,
            f"manifest.suites[{index}]",
        )
        precondition_value = suite.get("precondition")
        precondition = (
            None
            if precondition_value is None
            else expect_str(
                precondition_value, f"manifest.suites[{index}].precondition"
            )
        )
        if precondition is not None and precondition not in V01_PRECONDITIONS:
            raise ValueError(f"manifest.suites[{index}].precondition is not recognized")
        required = expect_bool(
            suite.get("required"),
            f"manifest.suites[{index}].required",
        )
        if required and precondition is None:
            participation: model.SuiteParticipation = model.BaselineParticipation()
        elif required and precondition is not None:
            participation = model.ConditionalParticipation(precondition)
        elif precondition is None:
            participation = model.OptionalParticipation()
        else:
            raise ValueError(
                f"manifest.suites[{index}].precondition is invalid when required is false"
            )
        suite_id = expect_str(suite.get("id"), f"manifest.suites[{index}].id")
        suite_file = expect_str(suite.get("file"), f"manifest.suites[{index}].file")
        if not is_canonical_relative_posix_path(suite_file):
            raise ValueError(
                f"manifest.suites[{index}].file must be a canonical relative POSIX path"
            )
        suites.append(
            model.SuiteEntry(
                suite_id=suite_id,
                file=suite_file,
                participation=participation,
            )
        )
    if not suites:
        raise ValueError("manifest.suites must not be empty")
    duplicate_ids = sorted(
        suite_id
        for suite_id, count in Counter(item.suite_id for item in suites).items()
        if count > 1
    )
    duplicate_files = sorted(
        suite_file
        for suite_file, count in Counter(item.file for item in suites).items()
        if count > 1
    )
    if duplicate_ids:
        raise ValueError(f"manifest.suites has duplicate ids {duplicate_ids}")
    if duplicate_files:
        raise ValueError(f"manifest.suites has duplicate files {duplicate_files}")
    return model.Manifest(
        version=version,
        release_history_root=release_history_root,
        release=release,
        status=status,
        root=suite_root,
        suites=tuple(suites),
    )


def parse_manifest(manifest_path: Path) -> model.Manifest:
    """Parse and narrow the conformance manifest."""
    return parse_manifest_value(
        load_yaml(manifest_path),
        str(manifest_path),
    )


def validate_manifest(
    repo_root: Path,
    suite_root: str,
    suites: list[model.SuiteEntry],
) -> list[ValidationProblem]:
    """Validate manifest-level invariants."""
    problems: list[ValidationProblem] = []
    suite_root_path = repo_root / suite_root
    if not suite_root_path.is_dir():
        problems.append(ValidationProblem(f"manifest root is missing: {suite_root}"))
        return problems

    id_counts = Counter(suite.suite_id for suite in suites)
    for suite_id, count in sorted(id_counts.items()):
        if count > 1:
            problems.append(ValidationProblem(f"duplicate suite id: {suite_id}"))

    file_counts = Counter(suite.file for suite in suites)
    for suite_file, count in sorted(file_counts.items()):
        if count > 1:
            problems.append(ValidationProblem(f"duplicate suite file: {suite_file}"))

    manifest_files = {suite.file for suite in suites}
    discovered_files = {
        str(path.relative_to(suite_root_path))
        for directory in SUITE_DIRECTORIES
        for path in (suite_root_path / directory).rglob("*.yaml")
        if path.name not in EXCLUDED_SUITE_FILES
    }
    for suite in suites:
        if (
            suite.precondition is not None
            and suite.precondition not in V01_PRECONDITIONS
        ):
            problems.append(
                ValidationProblem(
                    f"{suite.suite_id} uses unknown precondition {suite.precondition}"
                )
            )
        if suite.file.startswith("/") or ".." in Path(suite.file).parts:
            problems.append(
                ValidationProblem(f"{suite.suite_id} uses non-relative suite file")
            )
        if not (suite_root_path / suite.file).is_file():
            problems.append(
                ValidationProblem(
                    f"{suite.suite_id} suite file is missing: {suite.file}"
                )
            )

    for suite_file in sorted(discovered_files - manifest_files):
        problems.append(
            ValidationProblem(f"suite file missing from manifest: {suite_file}")
        )
    for suite_file in sorted(manifest_files - discovered_files):
        problems.append(
            ValidationProblem(f"manifest references unknown suite file: {suite_file}")
        )

    return problems


def validate_string_list(
    value: YamlValue,
    context: str,
    require_non_empty: bool,
) -> list[ValidationProblem]:
    """Validate a list of strings and return all problems."""
    problems: list[ValidationProblem] = []
    if not isinstance(value, list):
        return [ValidationProblem(f"{context} must be a list")]
    if require_non_empty and not value:
        problems.append(ValidationProblem(f"{context} must be non-empty"))
    for index, item in enumerate(value):
        if not isinstance(item, str):
            problems.append(ValidationProblem(f"{context}[{index}] must be a string"))
        elif require_non_empty and item == "":
            problems.append(ValidationProblem(f"{context}[{index}] must be non-empty"))
    return problems


def validate_diagnostic_assertion(
    diag: YamlValue,
    context: str,
    *,
    allow_primary_span: bool = False,
    channel: str | None = None,
    channel_registry: DiagnosticRegistry | None = None,
) -> list[ValidationProblem]:
    """Validate one structured diagnostic assertion."""
    problems: list[ValidationProblem] = []
    if not isinstance(diag, dict):
        return [ValidationProblem(f"{context} must be a mapping")]
    mapping = expect_mapping(diag, context)

    supported_fields = DIAGNOSTIC_SUPPORTED_FIELDS | (
        COMPILE_DIAGNOSTIC_FIELDS if allow_primary_span else set()
    )

    code = mapping.get("code")
    if not isinstance(code, str) or code == "":
        problems.append(ValidationProblem(f"{context}.code must be a non-empty string"))
    elif channel_registry is not None:
        registry_entry = channel_registry.get(code)
        if registry_entry is None:
            problems.append(
                ValidationProblem(f"{context}.code uses unknown diagnostic code {code}")
            )
        elif channel is None:
            problems.append(
                ValidationProblem(
                    f"{context} requires a channel when a diagnostic registry is supplied"
                )
            )
        elif channel not in registry_entry.channels.allowed_channels:
            problems.append(
                ValidationProblem(
                    f"{context}.code {code} is not allowed in {channel}; "
                    "allowed channels are "
                    f"{sorted(registry_entry.channels.allowed_channels)}"
                )
            )

    for key in sorted(set(mapping) - supported_fields):
        problems.append(ValidationProblem(f"{context}.{key} is not a supported key"))

    for field in sorted(DIAGNOSTIC_REQUIRED_LIST_FIELDS):
        if field not in mapping:
            problems.append(ValidationProblem(f"{context}.{field} is required"))
            continue
        problems.extend(
            validate_string_list(mapping[field], f"{context}.{field}", False)
        )

    for field in sorted(DIAGNOSTIC_OPTIONAL_LIST_FIELDS & set(mapping)):
        problems.extend(
            validate_string_list(mapping[field], f"{context}.{field}", True)
        )

    if "message_contains_any" in mapping:
        raw_groups = mapping["message_contains_any"]
        if not isinstance(raw_groups, list):
            problems.append(
                ValidationProblem(f"{context}.message_contains_any must be a list")
            )
        else:
            for index, group in enumerate(raw_groups):
                problems.extend(
                    validate_string_list(
                        group,
                        f"{context}.message_contains_any[{index}]",
                        True,
                    )
                )

    raw_data_keys = mapping.get("data_keys", [])
    data_keys = (
        {item for item in raw_data_keys if isinstance(item, str)}
        if isinstance(raw_data_keys, list)
        else set()
    )
    data_equals = mapping.get("data_equals")
    if data_equals is not None:
        if not isinstance(data_equals, dict):
            problems.append(
                ValidationProblem(f"{context}.data_equals must be a mapping")
            )
        else:
            for key, value in expect_mapping(
                data_equals, f"{context}.data_equals"
            ).items():
                if key not in data_keys:
                    problems.append(
                        ValidationProblem(
                            f"{context}.data_equals.{key} is not listed in data_keys"
                        )
                    )
                if not diagnostic_contracts.is_diagnostic_scalar(value):
                    problems.append(
                        ValidationProblem(
                            f"{context}.data_equals.{key} must be Str, i64, or Bool"
                        )
                    )

    if allow_primary_span and "primary_span" in mapping:
        raw_primary_span = mapping["primary_span"]
        if not isinstance(raw_primary_span, dict) or not all(
            isinstance(key, str) for key in raw_primary_span
        ):
            problems.append(
                ValidationProblem(
                    f"{context}.primary_span must be a string-keyed mapping"
                )
            )
        else:
            primary_span = expect_mapping(raw_primary_span, f"{context}.primary_span")
            keys = set(primary_span)
            if keys == _PRIMARY_MARKER_SPAN_FIELDS:
                marker = primary_span.get("marker")
                if (
                    not isinstance(marker, str)
                    or RE_PRIMARY_MARKER_NAME.fullmatch(marker) is None
                ):
                    problems.append(
                        ValidationProblem(
                            f"{context}.primary_span.marker must be a lowercase "
                            "kebab-case marker name"
                        )
                    )
            elif keys == _PRIMARY_BYTE_SPAN_FIELDS:
                source = primary_span.get("source")
                if not isinstance(source, str) or source == "":
                    problems.append(
                        ValidationProblem(
                            f"{context}.primary_span.source must be a non-empty string"
                        )
                    )
                else:
                    try:
                        source.encode("utf-8")
                    except UnicodeEncodeError:
                        problems.append(
                            ValidationProblem(
                                f"{context}.primary_span.source must contain only "
                                "Unicode scalar values"
                            )
                        )

                raw_bytes = primary_span.get("bytes")
                if not isinstance(raw_bytes, dict) or not all(
                    isinstance(key, str) for key in raw_bytes
                ):
                    problems.append(
                        ValidationProblem(
                            f"{context}.primary_span.bytes must be a string-keyed mapping"
                        )
                    )
                else:
                    byte_range = expect_mapping(
                        raw_bytes,
                        f"{context}.primary_span.bytes",
                    )
                    if set(byte_range) != _PRIMARY_BYTE_RANGE_FIELDS:
                        problems.append(
                            ValidationProblem(
                                f"{context}.primary_span.bytes must contain exactly "
                                "start and end"
                            )
                        )
                    start = byte_range.get("start")
                    end = byte_range.get("end")
                    for field, value in (("start", start), ("end", end)):
                        if (
                            type(value) is not int
                            or value < 0
                            or value > diagnostic_model.I64_MAX
                        ):
                            problems.append(
                                ValidationProblem(
                                    f"{context}.primary_span.bytes.{field} must be a "
                                    "non-negative i64"
                                )
                            )
                    if (
                        type(start) is int
                        and type(end) is int
                        and 0 <= start <= diagnostic_model.I64_MAX
                        and 0 <= end <= diagnostic_model.I64_MAX
                        and end < start
                    ):
                        problems.append(
                            ValidationProblem(
                                f"{context}.primary_span.bytes.end must not precede start"
                            )
                        )
            else:
                problems.append(
                    ValidationProblem(
                        f"{context}.primary_span must contain exactly marker or "
                        "source and bytes"
                    )
                )

    return problems


def validate_value_assertion(value: YamlValue, context: str) -> list[ValidationProblem]:
    """Validate an `expect.value` assertion."""
    problems: list[ValidationProblem] = []
    if not isinstance(value, dict):
        return [ValidationProblem(f"{context} must be a mapping")]
    mapping = expect_mapping(value, context)
    for key in sorted(set(mapping) - VALUE_ASSERTION_KEYS):
        problems.append(ValidationProblem(f"{context}.{key} is not a supported key"))
    if not isinstance(mapping.get("kind"), str) or mapping.get("kind") == "":
        problems.append(ValidationProblem(f"{context}.kind must be a non-empty string"))
    if "repr" in mapping and not isinstance(mapping["repr"], str):
        problems.append(ValidationProblem(f"{context}.repr must be a string"))
    error_variant = mapping.get("error_variant")
    if error_variant is not None:
        if mapping.get("kind") != "Err":
            problems.append(
                ValidationProblem(
                    f"{context}.error_variant is only valid for Err values"
                )
            )
        if not isinstance(error_variant, str) or error_variant == "":
            problems.append(
                ValidationProblem(f"{context}.error_variant must be a non-empty string")
            )
    numeric_compare = mapping.get("numeric_compare")
    if numeric_compare is not None:
        if not isinstance(numeric_compare, dict):
            problems.append(
                ValidationProblem(f"{context}.numeric_compare must be a mapping")
            )
        else:
            numeric = expect_mapping(numeric_compare, f"{context}.numeric_compare")
            for field in sorted(set(numeric) - NUMERIC_COMPARE_KEYS):
                problems.append(
                    ValidationProblem(
                        f"{context}.numeric_compare.{field} is not a supported key"
                    )
                )
            for field in ["dtype", "expected"]:
                if field not in numeric:
                    problems.append(
                        ValidationProblem(
                            f"{context}.numeric_compare.{field} is required"
                        )
                    )
            expected = numeric.get("expected")
            if "expected" in numeric:
                if isinstance(expected, list):
                    for index, item in enumerate(expected):
                        if isinstance(item, list | dict) or item is None:
                            problems.append(
                                ValidationProblem(
                                    f"{context}.numeric_compare.expected[{index}] "
                                    "must be a scalar"
                                )
                            )
                elif isinstance(expected, dict) or expected is None:
                    problems.append(
                        ValidationProblem(
                            f"{context}.numeric_compare.expected must be a "
                            "scalar or flat list"
                        )
                    )
            dtype = numeric.get("dtype")
            if dtype is not None and (not isinstance(dtype, str) or dtype == ""):
                problems.append(
                    ValidationProblem(
                        f"{context}.numeric_compare.dtype must be a non-empty string"
                    )
                )
            if isinstance(numeric.get("expected"), list) and "shape" not in numeric:
                problems.append(
                    ValidationProblem(
                        f"{context}.numeric_compare.shape is required for tensor values"
                    )
                )
            shape = numeric.get("shape")
            valid_shape: list[int] | None = None
            if shape is not None:
                if not isinstance(shape, list):
                    problems.append(
                        ValidationProblem(
                            f"{context}.numeric_compare.shape must be a list"
                        )
                    )
                else:
                    valid_shape = []
                    for index, dimension in enumerate(shape):
                        if type(dimension) is not int or dimension < 0:
                            problems.append(
                                ValidationProblem(
                                    f"{context}.numeric_compare.shape[{index}] "
                                    "must be a non-negative integer"
                                )
                            )
                        else:
                            valid_shape.append(dimension)
            if isinstance(expected, list) and valid_shape is not None:
                expected_length = 1
                for dimension in valid_shape:
                    expected_length *= dimension
                if len(expected) != expected_length:
                    problems.append(
                        ValidationProblem(
                            f"{context}.numeric_compare.expected length "
                            f"{len(expected)} does not match shape element "
                            f"count {expected_length}"
                        )
                    )
            mode = numeric.get("mode")
            if mode is not None and (
                not isinstance(mode, str) or mode not in NUMERIC_COMPARE_MODES
            ):
                problems.append(
                    ValidationProblem(
                        f"{context}.numeric_compare.mode must be one of "
                        f"{sorted(NUMERIC_COMPARE_MODES)}"
                    )
                )
            for field in ("rtol", "atol"):
                tolerance = numeric.get(field)
                if tolerance is not None and (
                    isinstance(tolerance, bool)
                    or not isinstance(tolerance, int | float)
                    or tolerance < 0
                ):
                    problems.append(
                        ValidationProblem(
                            f"{context}.numeric_compare.{field} must be a "
                            "non-negative number"
                        )
                    )
    return problems


def test_ref_key(ref: dict[str, YamlValue]) -> tuple[str, str] | None:
    """Return a stable test-result reference key when possible."""
    module = ref.get("module")
    name = ref.get("name")
    if isinstance(module, str) and isinstance(name, str):
        return module, name
    return None


def validate_test_ref(value: YamlValue, context: str) -> list[ValidationProblem]:
    """Validate one test-runner result reference."""
    problems: list[ValidationProblem] = []
    if not isinstance(value, dict):
        return [ValidationProblem(f"{context} must be a mapping")]
    mapping = expect_mapping(value, context)
    unknown = set(mapping) - {"module", "name"}
    for key in sorted(unknown):
        problems.append(ValidationProblem(f"{context}.{key} is not a supported key"))
    for field in ["module", "name"]:
        if not isinstance(mapping.get(field), str) or mapping.get(field) == "":
            problems.append(
                ValidationProblem(f"{context}.{field} must be a non-empty string")
            )
    return problems


def validate_test_ref_fields(
    mapping: dict[str, YamlValue], context: str
) -> list[ValidationProblem]:
    """Validate only the common module/name fields of a test-result reference."""
    problems: list[ValidationProblem] = []
    for field in ["module", "name"]:
        if not isinstance(mapping.get(field), str) or mapping.get(field) == "":
            problems.append(
                ValidationProblem(f"{context}.{field} must be a non-empty string")
            )
    return problems


def validate_test_ref_list(value: YamlValue, context: str) -> list[ValidationProblem]:
    """Validate a list of test-runner result references."""
    problems: list[ValidationProblem] = []
    if not isinstance(value, list):
        return [ValidationProblem(f"{context} must be a list")]
    seen: set[tuple[str, str]] = set()
    for index, item in enumerate(value):
        item_context = f"{context}[{index}]"
        problems.extend(validate_test_ref(item, item_context))
        if isinstance(item, dict):
            key = test_ref_key(expect_mapping(item, item_context))
            if key is not None:
                if key in seen:
                    problems.append(
                        ValidationProblem(
                            f"{item_context} duplicates {key[0]}::{key[1]}"
                        )
                    )
                seen.add(key)
    return problems


def validate_test_failure(
    value: YamlValue,
    context: str,
    channel_registry: DiagnosticRegistry | None = None,
) -> list[ValidationProblem]:
    """Validate one expected test-runner failure result."""
    problems: list[ValidationProblem] = []
    if not isinstance(value, dict):
        return [ValidationProblem(f"{context} must be a mapping")]
    mapping = expect_mapping(value, context)
    unknown = set(mapping) - {
        "module",
        "name",
        "kind",
        "message_contains",
        "runtime_code",
    }
    for key in sorted(unknown):
        problems.append(ValidationProblem(f"{context}.{key} is not a supported key"))
    problems.extend(validate_test_ref_fields(mapping, context))
    kind = mapping.get("kind")
    if kind not in TEST_RUN_FAILURE_KINDS:
        problems.append(
            ValidationProblem(
                f"{context}.kind must be one of {sorted(TEST_RUN_FAILURE_KINDS)}"
            )
        )
    if "message_contains" not in mapping:
        problems.append(ValidationProblem(f"{context}.message_contains is required"))
    else:
        problems.extend(
            validate_string_list(
                mapping["message_contains"],
                f"{context}.message_contains",
                True,
            )
        )
    runtime_code = mapping.get("runtime_code")
    if kind == "runtime_abort":
        if not isinstance(runtime_code, str) or runtime_code == "":
            problems.append(
                ValidationProblem(f"{context}.runtime_code must be a non-empty string")
            )
        elif channel_registry is not None:
            registry_entry = channel_registry.get(runtime_code)
            if registry_entry is None:
                problems.append(
                    ValidationProblem(
                        f"{context}.runtime_code uses unknown diagnostic code "
                        f"{runtime_code}"
                    )
                )
            elif (
                diagnostic_model.RUNTIME_ERROR
                not in registry_entry.channels.allowed_channels
            ):
                problems.append(
                    ValidationProblem(
                        f"{context}.runtime_code {runtime_code} is not allowed "
                        "in runtime_error; allowed channels are "
                        f"{sorted(registry_entry.channels.allowed_channels)}"
                    )
                )
    elif runtime_code is not None:
        problems.append(
            ValidationProblem(f"{context}.runtime_code is valid only for runtime_abort")
        )
    return problems


def validate_test_failures(
    value: YamlValue,
    context: str,
    channel_registry: DiagnosticRegistry | None = None,
) -> list[ValidationProblem]:
    """Validate a list of expected test-runner failures."""
    problems: list[ValidationProblem] = []
    if not isinstance(value, list):
        return [ValidationProblem(f"{context} must be a list")]
    seen: set[tuple[str, str]] = set()
    for index, item in enumerate(value):
        item_context = f"{context}[{index}]"
        problems.extend(
            validate_test_failure(
                item,
                item_context,
                channel_registry=channel_registry,
            )
        )
        if isinstance(item, dict):
            key = test_ref_key(expect_mapping(item, item_context))
            if key is not None:
                if key in seen:
                    problems.append(
                        ValidationProblem(
                            f"{item_context} duplicates {key[0]}::{key[1]}"
                        )
                    )
                seen.add(key)
    return problems


def keys_from_refs(items: YamlValue) -> set[tuple[str, str]]:
    """Return valid reference keys from a test-runner reference list."""
    if not isinstance(items, list):
        return set()
    keys: set[tuple[str, str]] = set()
    for item in items:
        if isinstance(item, dict):
            key = test_ref_key(expect_mapping(item, "test run reference"))
            if key is not None:
                keys.add(key)
    return keys


def keys_from_failures(items: YamlValue) -> set[tuple[str, str]]:
    """Return valid reference keys from a test-runner failure list."""
    return keys_from_refs(items)


def validate_test_run_assertion(
    value: YamlValue,
    context: str,
    channel_registry: DiagnosticRegistry | None = None,
) -> list[ValidationProblem]:
    """Validate an `expect.test_run` assertion."""
    problems: list[ValidationProblem] = []
    if not isinstance(value, dict):
        return [ValidationProblem(f"{context} must be a mapping")]
    mapping = expect_mapping(value, context)
    required_fields = {"order", "passed", "skipped", "failed"}
    for field in sorted(required_fields - set(mapping)):
        problems.append(ValidationProblem(f"{context}.{field} is required"))
    unknown = set(mapping) - required_fields
    for key in sorted(unknown):
        problems.append(ValidationProblem(f"{context}.{key} is not a supported key"))

    if "order" in mapping:
        problems.extend(validate_test_ref_list(mapping["order"], f"{context}.order"))
    if "passed" in mapping:
        problems.extend(validate_test_ref_list(mapping["passed"], f"{context}.passed"))
    if "skipped" in mapping:
        problems.extend(
            validate_test_ref_list(mapping["skipped"], f"{context}.skipped")
        )
    if "failed" in mapping:
        problems.extend(
            validate_test_failures(
                mapping["failed"],
                f"{context}.failed",
                channel_registry=channel_registry,
            )
        )

    order_keys = keys_from_refs(mapping.get("order"))
    partition_keys = (
        keys_from_refs(mapping.get("passed"))
        | keys_from_refs(mapping.get("skipped"))
        | keys_from_failures(mapping.get("failed"))
    )
    for key in sorted(partition_keys - order_keys):
        problems.append(
            ValidationProblem(
                f"{context} result {key[0]}::{key[1]} is not listed in order"
            )
        )
    for key in sorted(order_keys - partition_keys):
        problems.append(
            ValidationProblem(
                f"{context}.order entry {key[0]}::{key[1]} has no result state"
            )
        )

    result_sets = {
        "passed": keys_from_refs(mapping.get("passed")),
        "skipped": keys_from_refs(mapping.get("skipped")),
        "failed": keys_from_failures(mapping.get("failed")),
    }
    labels = sorted(result_sets)
    for index, left in enumerate(labels):
        for right in labels[index + 1 :]:
            overlap = result_sets[left] & result_sets[right]
            for key in sorted(overlap):
                problems.append(
                    ValidationProblem(
                        f"{context} result {key[0]}::{key[1]} appears in both "
                        f"{left} and {right}"
                    )
                )
    return problems


def validate_expect(
    expect: YamlValue,
    context: str,
    channel_registry: DiagnosticRegistry | None = None,
) -> list[ValidationProblem]:
    """Validate an `expect` mapping."""
    problems: list[ValidationProblem] = []
    if not isinstance(expect, dict):
        return [ValidationProblem(f"{context} must be a mapping")]
    mapping = expect_mapping(expect, context)
    primary = [key for key in PRIMARY_OUTCOMES if key in mapping]
    if len(primary) != 1:
        problems.append(
            ValidationProblem(
                f"{context} must contain exactly one primary outcome, found {primary}"
            )
        )
    unknown_keys = set(mapping) - PRIMARY_OUTCOMES - SECONDARY_EXPECT_KEYS
    for key in sorted(unknown_keys):
        problems.append(ValidationProblem(f"{context}.{key} is not a supported key"))

    if "compile_error" in mapping:
        problems.extend(
            validate_diagnostic_assertion(
                mapping["compile_error"],
                f"{context}.compile_error",
                allow_primary_span=True,
                channel=diagnostic_model.COMPILE_ERROR,
                channel_registry=channel_registry,
            )
        )
    if "runtime_error" in mapping:
        problems.extend(
            validate_diagnostic_assertion(
                mapping["runtime_error"],
                f"{context}.runtime_error",
                channel=diagnostic_model.RUNTIME_ERROR,
                channel_registry=channel_registry,
            )
        )
    if "value" in mapping:
        problems.extend(validate_value_assertion(mapping["value"], f"{context}.value"))
        value = mapping["value"]
        if isinstance(value, dict):
            value_mapping = expect_mapping(value, f"{context}.value")
            process_exit = mapping.get("process_exit")
            unit_process_assertion = (
                value_mapping.get("kind") == "Unit"
                and type(process_exit) is int
                and process_exit == 0
            )
            if (
                "repr" not in value_mapping
                and "numeric_compare" not in value_mapping
                and "value_diagnostics" not in mapping
                and not unit_process_assertion
            ):
                problems.append(
                    ValidationProblem(
                        f"{context}.value must include repr, numeric_compare, "
                        "value_diagnostics, or kind Unit with process_exit 0"
                    )
                )
    if "test_run" in mapping:
        problems.extend(
            validate_test_run_assertion(
                mapping["test_run"],
                f"{context}.test_run",
                channel_registry=channel_registry,
            )
        )
    if "diagnostic_contains" in mapping and "value" not in mapping:
        problems.append(
            ValidationProblem(f"{context}.diagnostic_contains is only valid with value")
        )
    if "diagnostic_contains" in mapping:
        problems.extend(
            validate_string_list(
                mapping["diagnostic_contains"],
                f"{context}.diagnostic_contains",
                True,
            )
        )
    if "side_effects" in mapping:
        problems.extend(
            validate_string_list(
                mapping["side_effects"],
                f"{context}.side_effects",
                False,
            )
        )
    if "value_diagnostics" in mapping:
        if "value" not in mapping:
            problems.append(
                ValidationProblem(
                    f"{context}.value_diagnostics is only valid with value"
                )
            )
        raw_diagnostics = mapping["value_diagnostics"]
        if not isinstance(raw_diagnostics, list):
            problems.append(
                ValidationProblem(f"{context}.value_diagnostics must be a list")
            )
        else:
            for index, diagnostic in enumerate(raw_diagnostics):
                problems.extend(
                    validate_diagnostic_assertion(
                        diagnostic,
                        f"{context}.value_diagnostics[{index}]",
                        channel=diagnostic_model.VALUE_DIAGNOSTICS,
                        channel_registry=channel_registry,
                    )
                )
    if "process_exit" in mapping:
        process_exit = mapping["process_exit"]
        if type(process_exit) is not int or process_exit not in {0, 1}:
            problems.append(
                ValidationProblem(f"{context}.process_exit must be integer 0 or 1")
            )

    return problems


def mask_strings_and_comments(source: str) -> str:
    """Mask strings and comments while preserving source offsets and newlines."""
    masked: list[str] = []
    in_string = False
    in_line_comment = False
    in_block_comment = False
    escaped = False
    index = 0

    while index < len(source):
        char = source[index]
        next_char = source[index + 1] if index + 1 < len(source) else ""

        if in_line_comment:
            if char == "\n":
                in_line_comment = False
                masked.append(char)
            else:
                masked.append(" ")
        elif in_block_comment:
            if char == "*" and next_char == "/":
                masked.extend((" ", " "))
                in_block_comment = False
                index += 1
            else:
                masked.append("\n" if char == "\n" else " ")
        elif in_string:
            masked.append("\n" if char == "\n" else " ")
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
        elif char == "/" and next_char == "/":
            masked.extend((" ", " "))
            in_line_comment = True
            index += 1
        elif char == "/" and next_char == "*":
            masked.extend((" ", " "))
            in_block_comment = True
            index += 1
        elif char == '"':
            masked.append(" ")
            in_string = True
        else:
            masked.append(char)

        index += 1

    return "".join(masked)


def local_pattern_names(pattern: str) -> tuple[str, ...]:
    """Extract names from the identifier and tuple patterns allowed for local lets."""
    if RE_IDENTIFIER.fullmatch(pattern):
        return () if pattern == "_" else (pattern,)
    if not (pattern.startswith("(") and pattern.endswith(")")):
        return ()

    remainder = RE_IDENTIFIER.sub("", pattern)
    if re.fullmatch(r"[\s(),]*", remainder) is None:
        return ()
    return tuple(name for name in RE_IDENTIFIER.findall(pattern) if name != "_")


def expected_compile_error_code(test: dict[str, YamlValue]) -> str | None:
    """Return the expected primary compile-error code, when present."""
    raw_expect = test.get("expect")
    if not isinstance(raw_expect, dict):
        return None
    raw_compile_error = raw_expect.get("compile_error")
    if not isinstance(raw_compile_error, dict):
        return None
    code = raw_compile_error.get("code")
    return code if isinstance(code, str) else None


def statement_token_starts(line: str) -> tuple[int, ...]:
    """Return first-token offsets for line and inline brace/lambda statements."""
    segment_starts = [0]
    segment_starts.extend(index + 1 for index, char in enumerate(line) if char == "{")

    token_starts: set[int] = set()
    for segment_start in segment_starts:
        token_start = segment_start
        while token_start < len(line) and line[token_start] in " \t":
            token_start += 1
        if token_start < len(line):
            token_starts.add(token_start)
        if token_start < len(line) and line[token_start] == "|":
            for index in range(token_start + 1, len(line)):
                if line[index] != "|":
                    continue
                lambda_body_start = index + 1
                while (
                    lambda_body_start < len(line) and line[lambda_body_start] in " \t"
                ):
                    lambda_body_start += 1
                if lambda_body_start < len(line):
                    token_starts.add(lambda_body_start)
    return tuple(sorted(token_starts))


def validate_eglur_assignments(
    source: str,
    context: str,
    allowed_assignment: tuple[str, str, str] | None,
) -> tuple[list[ValidationProblem], bool]:
    """Reject accidental bare assignment while preserving intentional diagnostics."""
    problems: list[ValidationProblem] = []
    scope_parents: list[int | None] = [None]
    scope_depths = [0]
    scope_stack = [0]
    paren_scope_depths: list[int] = []
    bracket_scope_depths: list[int] = []
    declarations: list[tuple[str, bool, int, int]] = []
    pending_declarations: list[tuple[tuple[str, ...], bool, int, int, int, int]] = []
    allowance_used = False
    source_offset = 0

    masked_source = mask_strings_and_comments(source)
    for line_number, raw_line in enumerate(masked_source.splitlines(keepends=True), 1):
        line = raw_line.rstrip("\r\n")
        token_starts = set(statement_token_starts(line))
        for index, char in enumerate(line):
            if index in token_starts:
                delimiter_scope_depth = max(
                    (*paren_scope_depths, *bracket_scope_depths),
                    default=-1,
                )
                if scope_depths[scope_stack[-1]] > delimiter_scope_depth:
                    remainder = line[index:]
                    declaration = RE_LET_DECLARATION.match(remainder)
                    if declaration is not None:
                        body = declaration.group("body")
                        pattern_end = min(
                            (
                                position
                                for position in (body.find(":"), body.find("="))
                                if position >= 0
                            ),
                            default=len(body),
                        )
                        names = local_pattern_names(body[:pattern_end].strip())
                        pending_declarations.append(
                            (
                                names,
                                declaration.group("mutable") is not None,
                                scope_stack[-1],
                                len(scope_stack),
                                len(paren_scope_depths),
                                len(bracket_scope_depths),
                            )
                        )
                    else:
                        typed_bare_binding = RE_TYPED_BARE_BINDING.match(remainder)
                        if typed_bare_binding is not None:
                            target = typed_bare_binding.group(1)
                            problems.append(
                                ValidationProblem(
                                    f"{context}:{line_number} typed bare binding "
                                    f"{target!r} requires a let declaration"
                                )
                            )

                        assignment = RE_IDENTIFIER_ASSIGNMENT.match(remainder)
                        if assignment is not None:
                            target = assignment.group(1)
                            if target != "_":
                                ancestor_scopes: set[int] = set()
                                scope_id: int | None = scope_stack[-1]
                                while scope_id is not None:
                                    ancestor_scopes.add(scope_id)
                                    scope_id = scope_parents[scope_id]
                                candidates = [
                                    binding
                                    for binding in declarations
                                    if binding[0] == target
                                    and binding[2] in ancestor_scopes
                                ]
                                binding_mutability = (
                                    max(
                                        candidates,
                                        key=lambda binding: (
                                            scope_depths[binding[2]],
                                            binding[3],
                                        ),
                                    )[1]
                                    if candidates
                                    else None
                                )
                                required_code = (
                                    "IMMUTABLE_BINDING_ASSIGN"
                                    if binding_mutability is False
                                    else "ASSIGN_TARGET_NOT_ASSIGNABLE"
                                )
                                is_allowed = (
                                    not allowance_used
                                    and allowed_assignment is not None
                                    and allowed_assignment[0] == target
                                    and allowed_assignment[2] == required_code
                                )
                                if binding_mutability is not True and not is_allowed:
                                    reason = (
                                        "an immutable local"
                                        if binding_mutability is False
                                        else "an unresolved identifier"
                                    )
                                    problems.append(
                                        ValidationProblem(
                                            f"{context}:{line_number} bare assignment "
                                            f"target {target!r} resolves to {reason}; "
                                            "use let/let mut or the discard target `_ =`"
                                        )
                                    )
                                elif binding_mutability is not True:
                                    allowance_used = True

            current_scope_depth = scope_depths[scope_stack[-1]]
            if char == "(":
                paren_scope_depths.append(current_scope_depth)
            elif char == ")" and paren_scope_depths:
                paren_scope_depths.pop()
            elif char == "[":
                bracket_scope_depths.append(current_scope_depth)
            elif char == "]" and bracket_scope_depths:
                bracket_scope_depths.pop()
            elif char == "{":
                scope_parents.append(scope_stack[-1])
                scope_depths.append(current_scope_depth + 1)
                scope_stack.append(len(scope_parents) - 1)
            elif char == "}" and len(scope_stack) > 1:
                scope_stack.pop()

        statement_end = source_offset + len(raw_line)
        remaining_pending: list[tuple[tuple[str, ...], bool, int, int, int, int]] = []
        for (
            names,
            mutable,
            declaration_scope,
            scope_count,
            paren_count,
            bracket_count,
        ) in pending_declarations:
            if (
                len(scope_stack) <= scope_count
                and len(paren_scope_depths) <= paren_count
                and len(bracket_scope_depths) <= bracket_count
            ):
                declarations.extend(
                    (name, mutable, declaration_scope, statement_end) for name in names
                )
            else:
                remaining_pending.append(
                    (
                        names,
                        mutable,
                        declaration_scope,
                        scope_count,
                        paren_count,
                        bracket_count,
                    )
                )
        pending_declarations = remaining_pending
        source_offset = statement_end

    return problems, allowance_used


def validate_assignment_targets(
    test: dict[str, YamlValue],
    context: str,
) -> list[ValidationProblem]:
    """Validate bare assignment targets in Eglur source forms."""
    sources: list[tuple[str, str]] = []
    raw_code = test.get("code")
    if isinstance(raw_code, str):
        sources.append((f"{context}.code", raw_code))

    raw_files = test.get("files")
    if isinstance(raw_files, dict):
        for rel_path, source in raw_files.items():
            if (
                isinstance(rel_path, str)
                and rel_path.endswith(".eglur")
                and isinstance(source, str)
            ):
                sources.append((f"{context}.files.{rel_path}", source))

    compile_error_code = expected_compile_error_code(test)
    raw_test_id = test.get("id")
    allowed_assignment = (
        INTENTIONAL_BARE_ASSIGNMENTS.get(raw_test_id)
        if isinstance(raw_test_id, str)
        else None
    )
    if allowed_assignment is not None and allowed_assignment[1] != compile_error_code:
        allowed_assignment = None
    problems: list[ValidationProblem] = []
    allowance_available = allowed_assignment
    for source_context, source in sources:
        source_problems, allowance_used = validate_eglur_assignments(
            source,
            source_context,
            allowance_available,
        )
        problems.extend(source_problems)
        if allowance_used:
            allowance_available = None
    return problems


def source_line_comments(source: str) -> list[tuple[int, str]]:
    """Return line comments outside string and block-comment lexical contexts."""
    source = source.replace("\r\n", "\n").replace("\r", "\n")
    comments: list[tuple[int, str]] = []
    in_block_comment = False
    in_string = False
    escaped = False
    line_number = 1
    index = 0

    while index < len(source):
        char = source[index]
        next_char = source[index + 1] if index + 1 < len(source) else ""

        if in_block_comment:
            if char == "*" and next_char == "/":
                in_block_comment = False
                index += 1
        elif in_string:
            if char == "\n":
                in_string = False
                escaped = False
            elif escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
        elif char == "/" and next_char == "*":
            in_block_comment = True
            index += 1
        elif char == "/" and next_char == "/":
            line_end = source.find("\n", index)
            if line_end == -1:
                line_end = len(source)
            comments.append((line_number, source[index:line_end]))
            index = line_end - 1
        elif char == '"':
            in_string = True

        if char == "\n":
            line_number += 1
        index += 1

    return comments


def source_primary_markers(
    test: dict[str, YamlValue],
) -> tuple[dict[str, list[tuple[str, int]]], list[tuple[str, int]]]:
    """Collect valid and malformed source-local primary markers."""
    sources: list[tuple[str, str]] = []
    raw_code = test.get("code")
    if isinstance(raw_code, str):
        sources.append(("code", raw_code))

    raw_files = test.get("files")
    if isinstance(raw_files, dict):
        for rel_path, source in raw_files.items():
            if (
                isinstance(rel_path, str)
                and rel_path.endswith(".eglur")
                and isinstance(source, str)
            ):
                sources.append((rel_path, source))

    markers: dict[str, list[tuple[str, int]]] = {}
    malformed: list[tuple[str, int]] = []
    for source_name, source in sources:
        for line_number, comment in source_line_comments(source):
            match = RE_PRIMARY_MARKER_COMMENT.fullmatch(comment)
            if match is not None:
                markers.setdefault(match.group("name"), []).append(
                    (source_name, line_number)
                )
                continue
            if RE_PRIMARY_MARKER_PREFIX.search(comment) is not None:
                malformed.append((source_name, line_number))
    return markers, malformed


def _fixture_source_byte_lengths(test: dict[str, YamlValue]) -> dict[str, int | None]:
    """Return source byte lengths, using None for malformed source encodings."""
    raw_code = test.get("code")
    if isinstance(raw_code, str):
        return {"code": _utf8_byte_length(raw_code)}

    raw_bytes = test.get("code_bytes_hex")
    if isinstance(raw_bytes, str):
        is_canonical_hex = (
            raw_bytes != ""
            and len(raw_bytes) % 2 == 0
            and all(character in "0123456789abcdefABCDEF" for character in raw_bytes)
        )
        return {"code": len(raw_bytes) // 2 if is_canonical_hex else None}

    raw_files = test.get("files")
    if not isinstance(raw_files, dict):
        return {}
    return {
        source: _utf8_byte_length(text)
        for source, text in raw_files.items()
        if isinstance(source, str)
        and source.endswith(".eglur")
        and isinstance(text, str)
    }


def _validate_primary_span_assertions(
    test: dict[str, YamlValue],
    context: str,
) -> list[ValidationProblem]:
    """Validate compile-error span assertions against fixture source units."""
    markers, malformed_markers = source_primary_markers(test)
    problems: list[ValidationProblem] = []
    raw_expect = test.get("expect")
    expected_marker: str | None = None
    expected_byte_span: tuple[str, int, int] | None = None
    if isinstance(raw_expect, dict):
        raw_compile_error = raw_expect.get("compile_error")
        if isinstance(raw_compile_error, dict):
            raw_primary_span = raw_compile_error.get("primary_span")
            if isinstance(raw_primary_span, dict):
                marker = raw_primary_span.get("marker")
                if (
                    isinstance(marker, str)
                    and RE_PRIMARY_MARKER_NAME.fullmatch(marker) is not None
                ):
                    expected_marker = marker
                source = raw_primary_span.get("source")
                raw_bytes = raw_primary_span.get("bytes")
                if (
                    isinstance(source, str)
                    and isinstance(raw_bytes, dict)
                    and all(isinstance(key, str) for key in raw_bytes)
                ):
                    byte_range = cast(dict[str, YamlValue], raw_bytes)
                    start = byte_range.get("start")
                    end = byte_range.get("end")
                    if type(start) is int and type(end) is int:
                        expected_byte_span = (source, start, end)

    for source_name, line_number in malformed_markers:
        problems.append(
            ValidationProblem(
                f"{context} has malformed primary marker at {source_name}:{line_number}"
            )
        )
    if expected_marker is not None:
        locations = markers.get(expected_marker, [])
        if not locations:
            problems.append(
                ValidationProblem(
                    f"{context}.expect.compile_error.primary_span.marker "
                    f"{expected_marker} is not declared in code or files"
                )
            )
        elif len(locations) > 1:
            rendered_locations = ", ".join(
                f"{source}:{line}" for source, line in locations
            )
            problems.append(
                ValidationProblem(
                    f"{context} primary marker {expected_marker} is declared "
                    f"more than once: {rendered_locations}"
                )
            )

    if expected_byte_span is not None:
        source, start, end = expected_byte_span
        source_lengths = _fixture_source_byte_lengths(test)
        source_len = source_lengths.get(source)
        if source not in source_lengths:
            problems.append(
                ValidationProblem(
                    f"{context}.expect.compile_error.primary_span.source {source!r} "
                    "does not identify a fixture source unit"
                )
            )
        if source_len is not None and 0 <= start <= end and end > source_len:
            problems.append(
                ValidationProblem(
                    f"{context}.expect.compile_error.primary_span.bytes ends at "
                    f"{end}, beyond source length {source_len}"
                )
            )
    for marker in sorted(set(markers) - {expected_marker}):
        problems.append(
            ValidationProblem(
                f"{context} source primary marker {marker} is not referenced by "
                "expect.compile_error.primary_span.marker"
            )
        )
    return problems


def validate_source_form(
    test: dict[str, YamlValue], context: str
) -> list[ValidationProblem]:
    """Validate source-form one-of constraints."""
    problems: list[ValidationProblem] = []
    has_code = "code" in test
    has_code_bytes_hex = "code_bytes_hex" in test
    has_files = "files" in test
    has_symlinks = "symlinks" in test
    has_registry_acquisitions = "registry_acquisitions" in test
    has_entry = "entry" in test
    has_file_set_root = "file_set_root" in test
    source_form_count = sum([has_code, has_code_bytes_hex, has_files])
    if source_form_count != 1:
        problems.append(
            ValidationProblem(
                f"{context} must use exactly one of code, code_bytes_hex, or files"
            )
        )
    if has_code:
        code = test.get("code")
        if not isinstance(code, str):
            problems.append(ValidationProblem(f"{context}.code must be a string"))
        elif _utf8_byte_length(code) is None:
            problems.append(
                ValidationProblem(f"{context}.code must be Unicode scalar text")
            )
        if has_entry:
            problems.append(ValidationProblem(f"{context}.entry is invalid with code"))
        if has_file_set_root:
            problems.append(
                ValidationProblem(f"{context}.file_set_root is invalid with code")
            )
        if has_symlinks:
            problems.append(
                ValidationProblem(f"{context}.symlinks is invalid with code")
            )
        if has_registry_acquisitions:
            problems.append(
                ValidationProblem(
                    f"{context}.registry_acquisitions is invalid with code"
                )
            )
    if has_code_bytes_hex:
        raw_bytes = test.get("code_bytes_hex")
        if not isinstance(raw_bytes, str):
            problems.append(
                ValidationProblem(f"{context}.code_bytes_hex must be a string")
            )
        else:
            is_hex = all(ch in "0123456789abcdefABCDEF" for ch in raw_bytes)
            if raw_bytes == "":
                problems.append(
                    ValidationProblem(f"{context}.code_bytes_hex must be non-empty")
                )
            if len(raw_bytes) % 2 != 0:
                problems.append(
                    ValidationProblem(f"{context}.code_bytes_hex length must be even")
                )
            if not is_hex:
                problems.append(
                    ValidationProblem(
                        f"{context}.code_bytes_hex must contain only hex digits"
                    )
                )
        if has_entry:
            problems.append(
                ValidationProblem(f"{context}.entry is invalid with code_bytes_hex")
            )
        if has_file_set_root:
            problems.append(
                ValidationProblem(
                    f"{context}.file_set_root is invalid with code_bytes_hex"
                )
            )
        if has_symlinks:
            problems.append(
                ValidationProblem(f"{context}.symlinks is invalid with code_bytes_hex")
            )
        if has_registry_acquisitions:
            problems.append(
                ValidationProblem(
                    f"{context}.registry_acquisitions is invalid with code_bytes_hex"
                )
            )
    if has_files:
        if not has_entry:
            problems.append(
                ValidationProblem(f"{context}.entry is required with files")
            )
        raw_files = test.get("files")
        if not isinstance(raw_files, dict) or not all(
            isinstance(key, str) for key in raw_files
        ):
            problems.append(
                ValidationProblem(f"{context}.files must be a string-keyed mapping")
            )
        else:
            files = cast(dict[str, YamlValue], raw_files)
            if has_symlinks:
                problems.extend(
                    _validate_symlink_topology(
                        test.get("symlinks"),
                        files,
                        context,
                    )
                )
            mode = test.get("mode", "auto")
            is_supported_mode = isinstance(mode, str) and mode in ALLOWED_MODES
            is_package_mode = is_supported_mode and (
                mode == "package" or (mode == "auto" and "Eglur.toml" in files)
            )
            is_file_set_mode = is_supported_mode and not is_package_mode

            if has_registry_acquisitions:
                if not is_package_mode:
                    problems.append(
                        ValidationProblem(
                            f"{context}.registry_acquisitions is valid only in "
                            "package mode"
                        )
                    )
                problems.extend(
                    _validate_registry_acquisitions(
                        test.get("registry_acquisitions"),
                        f"{context}.registry_acquisitions",
                    )
                )

            if not files:
                problems.append(ValidationProblem(f"{context}.files must be non-empty"))
            for rel_path, source in files.items():
                is_scalar_path = _utf8_byte_length(rel_path) is not None
                path_parts = Path(rel_path).parts
                is_invalid_path = (
                    rel_path.startswith("/")
                    or "\\" in rel_path
                    or ".." in path_parts
                    or not is_scalar_path
                    or (
                        is_file_set_mode
                        and not is_canonical_relative_posix_path(rel_path)
                    )
                )
                if is_invalid_path:
                    rendered_path = rel_path if is_scalar_path else ascii(rel_path)
                    problems.append(
                        ValidationProblem(
                            f"{context}.files has invalid path {rendered_path}"
                        )
                    )
                if not isinstance(source, str):
                    problems.append(
                        ValidationProblem(
                            f"{context}.files.{rel_path} must be a string"
                        )
                    )
                elif _utf8_byte_length(source) is None:
                    problems.append(
                        ValidationProblem(
                            f"{context}.files.{rel_path} must be Unicode scalar text"
                        )
                    )
            entry = test.get("entry")
            if not isinstance(entry, str) or entry == "":
                problems.append(
                    ValidationProblem(f"{context}.entry must be a non-empty string")
                )
            elif _utf8_byte_length(entry) is None:
                problems.append(
                    ValidationProblem(f"{context}.entry must be Unicode scalar text")
                )
            elif entry not in files:
                problems.append(
                    ValidationProblem(f"{context}.entry must be present in files")
                )
            elif not entry.endswith(".eglur"):
                problems.append(
                    ValidationProblem(
                        f"{context}.entry must designate an .eglur source"
                    )
                )

            if is_supported_mode:
                source_paths = [
                    rel_path for rel_path in files if rel_path.endswith(".eglur")
                ]

                if is_package_mode:
                    if has_file_set_root:
                        problems.append(
                            ValidationProblem(
                                f"{context}.file_set_root is invalid in package mode"
                            )
                        )
                    has_package_source = any(
                        source_path.startswith("src/") for source_path in source_paths
                    )
                    if (
                        has_package_source
                        and isinstance(entry, str)
                        and entry in files
                        and not entry.startswith("src/")
                    ):
                        problems.append(
                            ValidationProblem(
                                f"{context}.entry must be under src/ in package mode"
                            )
                        )
                elif len(source_paths) <= 1:
                    if has_file_set_root:
                        problems.append(
                            ValidationProblem(
                                f"{context}.file_set_root is invalid for a "
                                "single-source file set"
                            )
                        )
                elif not has_file_set_root:
                    problems.append(
                        ValidationProblem(
                            f"{context}.file_set_root is required for a "
                            "multi-source file set"
                        )
                    )
                else:
                    file_set_root = test.get("file_set_root")
                    if not isinstance(file_set_root, str) or not (
                        is_canonical_relative_posix_path(
                            file_set_root,
                            allow_current_directory=True,
                        )
                    ):
                        problems.append(
                            ValidationProblem(
                                f"{context}.file_set_root must be a canonical "
                                "relative POSIX path"
                            )
                        )
                    else:
                        root_prefix = (
                            "" if file_set_root == "." else f"{file_set_root}/"
                        )
                        for source_path in source_paths:
                            if not source_path.startswith(root_prefix):
                                problems.append(
                                    ValidationProblem(
                                        f"{context} source {source_path} is outside "
                                        f"file_set_root {file_set_root}"
                                    )
                                )
    return problems


def validate_test_case(
    raw_test: YamlValue,
    suite_file: str,
    index: int,
    channel_registry: DiagnosticRegistry | None = None,
) -> tuple[str | None, list[ValidationProblem]]:
    """Validate one test case and return its ID when available."""
    context = f"{suite_file}.tests[{index}]"
    problems: list[ValidationProblem] = []
    if not isinstance(raw_test, dict):
        return None, [ValidationProblem(f"{context} must be a mapping")]
    test = expect_mapping(raw_test, context)
    for key in sorted(set(test) - TEST_CASE_KEYS):
        problems.append(ValidationProblem(f"{context}.{key} is not a supported key"))

    test_id: str | None = None
    for field in ["id", "category", "title"]:
        value = test.get(field)
        if not isinstance(value, str) or value == "":
            problems.append(
                ValidationProblem(f"{context}.{field} must be a non-empty string")
            )
        elif field == "id":
            test_id = value

    mode = test.get("mode", "auto")
    if not isinstance(mode, str) or mode not in ALLOWED_MODES:
        problems.append(ValidationProblem(f"{context}.mode is unsupported: {mode}"))
    runner = test.get("runner", "main")
    if runner not in ALLOWED_RUNNERS:
        problems.append(ValidationProblem(f"{context}.runner is unsupported: {runner}"))

    problems.extend(validate_source_form(test, context))
    problems.extend(validate_assignment_targets(test, context))
    problems.extend(_validate_primary_span_assertions(test, context))
    if "expect" not in test:
        problems.append(ValidationProblem(f"{context}.expect is required"))
    else:
        problems.extend(
            validate_expect(
                test["expect"],
                f"{context}.expect",
                channel_registry=channel_registry,
            )
        )
        if isinstance(test["expect"], dict):
            expect = expect_mapping(test["expect"], f"{context}.expect")
            if "test_run" in expect and runner != "test":
                problems.append(
                    ValidationProblem(
                        f"{context}.runner must be test for expect.test_run"
                    )
                )
            if runner == "test" and "test_run" not in expect:
                problems.append(
                    ValidationProblem(
                        f"{context}.expect.test_run is required for runner test"
                    )
                )
            if "process_exit" in expect:
                process_exit = expect["process_exit"]
                if runner != "main":
                    problems.append(
                        ValidationProblem(
                            f"{context}.expect.process_exit is only valid for runner main"
                        )
                    )
                if "compile_error" in expect or "test_run" in expect:
                    problems.append(
                        ValidationProblem(
                            f"{context}.expect.process_exit is invalid with "
                            "compile_error or test_run"
                        )
                    )
                if (
                    "runtime_error" in expect
                    and type(process_exit) is int
                    and process_exit != 1
                ):
                    problems.append(
                        ValidationProblem(
                            f"{context}.expect.process_exit must be 1 with runtime_error"
                        )
                    )
                value = expect.get("value")
                if isinstance(value, dict):
                    value_mapping = expect_mapping(value, f"{context}.expect.value")
                    value_kind = value_mapping.get("kind")
                    exit_by_kind = {"Unit": 0, "Ok": 0, "Err": 1}
                    expected_exit = (
                        exit_by_kind.get(value_kind)
                        if isinstance(value_kind, str)
                        else None
                    )
                    if expected_exit is None:
                        problems.append(
                            ValidationProblem(
                                f"{context}.expect.process_exit requires value kind "
                                "Unit, Ok, or Err"
                            )
                        )
                    elif type(process_exit) is int and process_exit != expected_exit:
                        problems.append(
                            ValidationProblem(
                                f"{context}.expect.process_exit must be "
                                f"{expected_exit} with value kind {value_kind}"
                            )
                        )

    return test_id, problems


def validate_suite_file(
    path: Path,
    rel_file: str,
    channel_registry: DiagnosticRegistry | None = None,
) -> tuple[list[str], list[ValidationProblem]]:
    """Validate one suite file and return discovered test IDs plus problems."""
    raw = load_yaml(path)
    root = expect_mapping(raw, rel_file)
    for key in sorted(set(root) - SUITE_FILE_KEYS):
        return [], [ValidationProblem(f"{rel_file}.{key} is not a supported key")]
    raw_tests = root.get("tests")
    problems: list[ValidationProblem] = []
    ids: list[str] = []
    if not isinstance(raw_tests, list):
        return ids, [ValidationProblem(f"{rel_file}.tests must be a list")]
    for index, raw_test in enumerate(raw_tests):
        test_id, test_problems = validate_test_case(
            raw_test,
            rel_file,
            index,
            channel_registry=channel_registry,
        )
        if test_id is not None:
            ids.append(test_id)
        problems.extend(test_problems)
    local_counts = Counter(ids)
    for test_id, count in sorted(local_counts.items()):
        if count > 1:
            problems.append(
                ValidationProblem(f"{rel_file} has duplicate test id {test_id}")
            )
    return ids, problems


def validate_all(repo_root: Path, manifest_path: Path) -> list[ValidationProblem]:
    """Validate the manifest and all manifest-listed suite files."""
    manifest = parse_manifest(manifest_path)
    problems = validate_manifest(
        repo_root,
        manifest.root,
        list(manifest.suites),
    )
    suite_root_path = repo_root / manifest.root
    contracts = diagnostic_contracts.load_contracts(
        repo_root / diagnostic_contracts.DEFAULT_CONTRACT_PATH
    )
    channel_registry = load_diagnostic_registry(
        repo_root / "docs/spec/errors",
        contracts,
    )

    seen_ids: dict[str, str] = {}
    duplicate_ids: list[tuple[str, str, str]] = []
    suite_files = sorted({suite.file for suite in manifest.suites})
    for suite_file in suite_files:
        suite_path = suite_root_path / suite_file
        if not suite_path.is_file():
            continue
        ids, suite_problems = validate_suite_file(
            suite_path,
            suite_file,
            channel_registry=channel_registry,
        )
        problems.extend(suite_problems)
        for test_id in ids:
            if test_id in seen_ids:
                duplicate_ids.append((test_id, seen_ids[test_id], suite_file))
            else:
                seen_ids[test_id] = suite_file

    for test_id, first_file, second_file in duplicate_ids:
        if first_file != second_file:
            problems.append(
                ValidationProblem(
                    f"global duplicate test id {test_id}: {first_file}, {second_file}"
                )
            )
    return problems

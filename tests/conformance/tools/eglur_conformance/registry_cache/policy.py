"""Validate registry-cache scenarios against manifest-owned source fixtures."""

import hashlib
import re
import tomllib
from dataclasses import dataclass
from datetime import date, datetime, time
from typing import TypeAlias

from eglur_conformance.registry_cache import model
from eglur_conformance.suite import model as suite_model
from eglur_conformance.validation import ValidationProblem

_TREE_INTEGRITY_PATTERN = re.compile(r"tree-sha256:[0-9a-f]{64}\Z")
_MAX_LOGICAL_ENTRY_COUNT = 65_536
_MAX_PATH_BYTES = 4_096
_MAX_LINK_TARGET_BYTES = 4_096
_MAX_TOTAL_PATH_AND_TARGET_BYTES = 16_777_216
_MAX_REGULAR_FILE_BYTES = 134_217_728
_MAX_TOTAL_REGULAR_FILE_BYTES = 536_870_912
_MAX_PATH_COMPONENT_POSITIONS = 128
_MASKED_REGISTRY_PACKAGE = "<registry-package>"
_MASKED_REGISTRY_PACKAGE_PATH = "<registry-package-path>"
_MASKED_REGISTRY_AUTHORITY = "<registry-authority>"
_MASKED_REGISTRY_VERSION = "<registry-version>"
_MASKED_REGISTRY_INTEGRITY = "<registry-integrity>"

TomlScalar: TypeAlias = str | int | float | bool | date | datetime | time
TomlValue: TypeAlias = TomlScalar | list["TomlValue"] | dict[str, "TomlValue"]


@dataclass(frozen=True)
class _TomlScalarFingerprint:
    kind: str
    text: str


@dataclass(frozen=True)
class _TomlArrayFingerprint:
    items: tuple["_TomlFingerprint", ...]


@dataclass(frozen=True)
class _TomlTableFingerprint:
    items: tuple[tuple[str, "_TomlFingerprint"], ...]


_TomlFingerprint: TypeAlias = (
    _TomlScalarFingerprint | _TomlArrayFingerprint | _TomlTableFingerprint
)


@dataclass(frozen=True)
class _RegistrySourceCoordinate:
    package: str
    version: str
    integrity: str


@dataclass(frozen=True)
class _LockedRegistryTarget:
    coordinate: _RegistrySourceCoordinate
    dependency_key: str
    package_name: str
    registry_node: str
    root_name: str
    root_version: str


@dataclass(frozen=True)
class _FixtureInputContext:
    root_manifest: _TomlTableFingerprint
    root_lock: _TomlTableFingerprint
    source_files: tuple[suite_model.SourceFile, ...]
    symlinks: tuple[suite_model.FixtureSymlink, ...]
    entry: str
    root: str | None
    mode: suite_model.FixtureMode
    runner: suite_model.FixtureRunner
    provider_steps: tuple[suite_model.RegistryProviderStep, ...]


@dataclass(frozen=True)
class _AccountedRegistryEntry:
    entry: suite_model.RegistryEntry
    path: bytes
    content: bytes | None


@dataclass(frozen=True)
class _FixtureFacts:
    coordinate: _RegistrySourceCoordinate
    dependency_key: str
    target_package_name: str
    target_node: str
    provider_package_metadata: tuple[str, str] | None
    provider_tree_integrity: str | None
    provider_finish: suite_model.RegistryProviderFinish
    outcome: suite_model.ExpectedOutcome
    input_context: _FixtureInputContext

    @property
    def compile_error_code(self) -> str | None:
        if isinstance(self.outcome, suite_model.CompileErrorOutcome):
            return self.outcome.diagnostic.code
        return None

    @property
    def compile_error_data_equals(self) -> dict[str, str | int | bool]:
        if isinstance(self.outcome, suite_model.CompileErrorOutcome):
            return dict(self.outcome.diagnostic.data_equals)
        return {}


def _toml_mapping(value: TomlValue, context: str) -> dict[str, TomlValue]:
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a TOML table")
    return value


def _toml_list(value: TomlValue | None, context: str) -> list[TomlValue]:
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a TOML array")
    return value


def _toml_text(value: TomlValue | None, context: str) -> str:
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a TOML string")
    if not value:
        raise ValueError(f"{context} must be non-empty")
    return value


def _toml_fingerprint(value: TomlValue) -> _TomlFingerprint:
    if isinstance(value, dict):
        return _TomlTableFingerprint(
            tuple((key, _toml_fingerprint(item)) for key, item in sorted(value.items()))
        )
    if isinstance(value, list):
        return _TomlArrayFingerprint(tuple(_toml_fingerprint(item) for item in value))
    text = (
        value.isoformat() if isinstance(value, date | datetime | time) else repr(value)
    )
    return _TomlScalarFingerprint(type(value).__name__, text)


def _toml_table_fingerprint(
    value: dict[str, TomlValue],
) -> _TomlTableFingerprint:
    fingerprint = _toml_fingerprint(value)
    if not isinstance(fingerprint, _TomlTableFingerprint):
        raise TypeError("TOML table fingerprint lost its table kind")
    return fingerprint


def _registry_target(
    lock_text: str,
    context: str,
) -> tuple[_LockedRegistryTarget, _TomlTableFingerprint]:
    document: TomlValue = tomllib.loads(lock_text)
    root = _toml_mapping(document, context)
    selected_root_node = _toml_text(root.get("root"), f"{context}.root")
    registry_records: list[dict[str, TomlValue]] = []
    root_records: list[dict[str, TomlValue]] = []
    records_by_node: dict[str, dict[str, TomlValue]] = {}
    for index, raw_record in enumerate(
        _toml_list(root.get("packages"), f"{context}.packages")
    ):
        record = _toml_mapping(raw_record, f"{context}.packages[{index}]")
        node = _toml_text(record.get("node"), f"{context}.packages[{index}].node")
        if node in records_by_node:
            raise ValueError(f"{context} contains duplicate package node {node!r}")
        records_by_node[node] = record
        if record.get("source") == "registry":
            registry_records.append(record)
        if record.get("source") == "root":
            root_records.append(record)
    if len(registry_records) != 1:
        raise ValueError(f"{context} must contain exactly one registry package record")
    if len(root_records) != 1:
        raise ValueError(f"{context} must contain exactly one root package record")
    record = registry_records[0]
    registry_node = _toml_text(record.get("node"), f"{context}.registry.node")
    root_record = root_records[0]
    root_node = _toml_text(root_record.get("node"), f"{context}.root.node")
    if root_node != selected_root_node:
        raise ValueError(
            f"{context} selected root does not name the root package record"
        )
    root_dependencies = _toml_mapping(
        root_record.get("dependencies", {}),
        f"{context}.root.dependencies",
    )
    matching_dependencies: list[str] = []
    for dependency, raw_edge in root_dependencies.items():
        edge = _toml_mapping(
            raw_edge,
            f"{context}.root.dependencies.{dependency}",
        )
        target_node = _toml_text(
            edge.get("target"),
            f"{context}.root.dependencies.{dependency}.target",
        )
        if target_node not in records_by_node:
            raise ValueError(
                f"{context}.root.dependencies.{dependency} targets an unknown node"
            )
        if target_node == registry_node:
            if set(edge) != {"target"}:
                raise ValueError(
                    f"{context}.root.dependencies.{dependency} must use the exact "
                    "registry edge schema"
                )
            matching_dependencies.append(dependency)
    if len(matching_dependencies) != 1:
        raise ValueError(
            f"{context} root must select the registry node through exactly one dependency"
        )
    coordinate = _RegistrySourceCoordinate(
        package=_toml_text(record.get("package"), f"{context}.package"),
        version=_toml_text(record.get("version"), f"{context}.version"),
        integrity=_toml_text(record.get("integrity"), f"{context}.integrity"),
    )
    if _TREE_INTEGRITY_PATTERN.fullmatch(coordinate.integrity) is None:
        raise ValueError(f"{context}.integrity must use canonical tree-sha256")
    target = _LockedRegistryTarget(
        coordinate=coordinate,
        dependency_key=matching_dependencies[0],
        package_name=_toml_text(record.get("name"), f"{context}.name"),
        registry_node=registry_node,
        root_name=_toml_text(root_record.get("name"), f"{context}.root.name"),
        root_version=_toml_text(
            root_record.get("version"),
            f"{context}.root.version",
        ),
    )
    record["package"] = _MASKED_REGISTRY_PACKAGE
    record["version"] = _MASKED_REGISTRY_VERSION
    record["integrity"] = _MASKED_REGISTRY_INTEGRITY
    return target, _toml_table_fingerprint(root)


def _package_metadata(manifest_text: str, context: str) -> tuple[str, str]:
    document: TomlValue = tomllib.loads(manifest_text)
    root = _toml_mapping(document, context)
    package = _toml_mapping(root.get("package", {}), f"{context}.package")
    return (
        _toml_text(package.get("name"), f"{context}.package.name"),
        _toml_text(package.get("version"), f"{context}.package.version"),
    )


def _validate_root_manifest(
    manifest_text: str,
    target: _LockedRegistryTarget,
    context: str,
) -> _TomlTableFingerprint:
    document: TomlValue = tomllib.loads(manifest_text)
    root = _toml_mapping(document, context)
    package = _toml_mapping(root.get("package", {}), f"{context}.package")
    root_metadata = (
        _toml_text(package.get("name"), f"{context}.package.name"),
        _toml_text(package.get("version"), f"{context}.package.version"),
    )
    if root_metadata != (target.root_name, target.root_version):
        raise ValueError(f"{context} root manifest does not match its lock record")

    dependencies = _toml_mapping(
        root.get("dependencies", {}),
        f"{context}.dependencies",
    )
    selector = _toml_mapping(
        dependencies.get(target.dependency_key, {}),
        f"{context}.dependencies.{target.dependency_key}",
    )
    selector_keys = set(selector)
    alias: str | None = None
    if selector_keys == {"package", "version"}:
        selected_package = _toml_text(
            selector.get("package"),
            f"{context}.dependencies.{target.dependency_key}.package",
        )
        masked_package = _MASKED_REGISTRY_PACKAGE
    elif selector_keys == {"registry", "package", "version"}:
        alias = _toml_text(
            selector.get("registry"),
            f"{context}.dependencies.{target.dependency_key}.registry",
        )
        registries = _toml_mapping(root.get("registries", {}), f"{context}.registries")
        authority = _toml_text(
            registries.get(alias),
            f"{context}.registries.{alias}",
        )
        package_path = _toml_text(
            selector.get("package"),
            f"{context}.dependencies.{target.dependency_key}.package",
        )
        selected_package = f"{authority}/{package_path}"
        registries[alias] = _MASKED_REGISTRY_AUTHORITY
        masked_package = _MASKED_REGISTRY_PACKAGE_PATH
    else:
        raise ValueError(
            f"{context}.dependencies.{target.dependency_key} must be one qualified "
            "or alias registry selector"
        )
    selected_coordinate = (
        selected_package,
        _toml_text(
            selector.get("version"),
            f"{context}.dependencies.{target.dependency_key}.version",
        ),
    )
    if selected_coordinate != (target.coordinate.package, target.coordinate.version):
        raise ValueError(
            f"{context}.dependencies.{target.dependency_key} does not select its "
            "locked registry coordinate"
        )
    selector["package"] = masked_package
    selector["version"] = _MASKED_REGISTRY_VERSION
    if alias is not None:
        selector["registry"] = alias
    return _toml_table_fingerprint(root)


def _provider_package_metadata(
    files: dict[bytes, bytes],
    context: str,
) -> tuple[str, str] | None:
    manifest = files.get(b"Eglur.toml")
    if manifest is None:
        return None
    return _package_metadata(
        manifest.decode("utf-8"),
        f"{context}.registry.Eglur.toml",
    )


def _exact_registry_bytes(
    value: suite_model.RegistryBytes,
    context: str,
) -> bytes:
    if not isinstance(value, bytes):
        raise TypeError(f"{context} must use exact bytes in registry-cache fixtures")
    return value


def _profile_path_text(path: bytes, context: str) -> str:
    try:
        text = path.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{context} must be valid UTF-8 path bytes") from error
    if not text.isascii():
        raise ValueError(
            f"{context} must use ASCII path bytes in registry-cache fixtures"
        )
    if (
        not text
        or text.startswith("/")
        or "\\" in text
        or "\0" in text
        or any(component in {"", ".", ".."} for component in text.split("/"))
    ):
        raise ValueError(f"{context} must be a canonical relative POSIX path")
    return text


def _accounted_provider_entries(
    acquisition: suite_model.RegistryAcquisition,
    context: str,
) -> tuple[_AccountedRegistryEntry, ...]:
    finish = acquisition.provider_steps[-1]
    if not isinstance(finish, suite_model.RegistryFinishStep):
        raise TypeError(f"{context} registry acquisition has no terminal finish")

    raw_entries = acquisition.provider_steps[:-1]
    total_path_and_target_bytes = 0
    total_regular_file_bytes = 0
    entries: list[_AccountedRegistryEntry] = []
    for index, step in enumerate(raw_entries):
        step_context = f"{context}.provider_steps[{index}]"
        if isinstance(step, suite_model.RegistryRepeatStep):
            raise TypeError(
                f"{step_context} must not repeat registry-cache fixture entries"
            )
        if not isinstance(step, suite_model.RegistryEmitStep):
            raise TypeError(f"{step_context} must emit an entry")
        entry_count = index + 1
        if entry_count > _MAX_LOGICAL_ENTRY_COUNT:
            raise ValueError(f"{step_context} exceeds the logical entry-count limit")
        entry = step.entry
        path = _exact_registry_bytes(
            entry.path,
            f"{step_context}.path",
        )
        if len(path) > _MAX_PATH_BYTES:
            raise ValueError(f"{step_context} exceeds the path-byte limit")
        component_positions = 0 if not path else path.count(b"/") + 1
        if component_positions > _MAX_PATH_COMPONENT_POSITIONS:
            raise ValueError(f"{step_context} exceeds the path-depth limit")

        link_target_bytes = 0
        if isinstance(entry, suite_model.RegistryLinkEntry):
            target = _exact_registry_bytes(entry.target, f"{step_context}.target")
            link_target_bytes = len(target)
            if link_target_bytes > _MAX_LINK_TARGET_BYTES:
                raise ValueError(f"{step_context} exceeds the link-target-byte limit")
        total_path_and_target_bytes += len(path) + link_target_bytes
        if total_path_and_target_bytes > _MAX_TOTAL_PATH_AND_TARGET_BYTES:
            raise ValueError(
                f"{step_context} exceeds the total path-and-target-byte limit"
            )

        content: bytes | None = None
        if isinstance(entry, suite_model.RegistryFileEntry):
            content = _exact_registry_bytes(entry.content, f"{step_context}.content")
            if len(content) > _MAX_REGULAR_FILE_BYTES:
                raise ValueError(f"{step_context} exceeds the regular-file-byte limit")
            total_regular_file_bytes += len(content)
            if total_regular_file_bytes > _MAX_TOTAL_REGULAR_FILE_BYTES:
                raise ValueError(
                    f"{step_context} exceeds the total regular-file-byte limit"
                )
        entries.append(
            _AccountedRegistryEntry(
                entry=entry,
                path=path,
                content=content,
            )
        )
    return tuple(entries)


def _validated_provider_files(
    entries: tuple[_AccountedRegistryEntry, ...],
    context: str,
) -> dict[bytes, bytes]:

    files: dict[bytes, bytes] = {}
    folded_paths: dict[str, bytes] = {}
    for index, accounted in enumerate(entries):
        step_context = f"{context}.provider_steps[{index}]"
        entry = accounted.entry
        path = accounted.path
        content = accounted.content
        path_text = _profile_path_text(path, f"{step_context}.path")
        if isinstance(entry, suite_model.RegistryDirectoryEntry):
            continue
        if not isinstance(entry, suite_model.RegistryFileEntry):
            raise TypeError(f"{step_context} must emit only files or directories")
        if content is None:
            raise TypeError(f"{step_context}.content must be exact bytes")
        if path in files:
            raise ValueError(f"{context} emits duplicate registry path {path!r}")
        folded_path = path_text.lower()
        colliding_path = folded_paths.get(folded_path)
        if colliding_path is not None:
            raise ValueError(
                f"{context} emits ASCII case-colliding registry paths "
                f"{colliding_path!r} and {path!r}"
            )
        folded_paths[folded_path] = path
        files[path] = content

    file_paths = set(files)
    for path in sorted(file_paths):
        components = path.split(b"/")
        for boundary in range(1, len(components)):
            ancestor = b"/".join(components[:boundary])
            if ancestor in file_paths:
                raise ValueError(
                    f"{context} emits file ancestor {ancestor!r} of {path!r}"
                )
    return files


def _provider_tree_integrity(files: dict[bytes, bytes]) -> str:
    digest = hashlib.sha256()
    digest.update(b"eglur-package-tree-v1\0")
    digest.update(len(files).to_bytes(8, "big"))
    for path, content in sorted(files.items()):
        digest.update(len(path).to_bytes(8, "big"))
        digest.update(path)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return f"tree-sha256:{digest.hexdigest()}"


def _fixture_facts(
    reference: suite_model.TestIdentity,
    test: suite_model.TestCase,
) -> _FixtureFacts:
    context = str(reference)
    invocation = test.invocation
    if invocation.mode != "package":
        raise ValueError(f"{context} must use package mode")
    if not isinstance(invocation.source, suite_model.FileSetSource):
        raise TypeError(f"{context} must use a multi-file package fixture")
    if len(invocation.registry_acquisitions) != 1:
        raise ValueError(f"{context} must declare exactly one registry acquisition")

    source_files = {item.path: item.text for item in invocation.source.files}
    lock_text = source_files.get("Eglur.lock")
    if lock_text is None:
        raise ValueError(f"{context} must contain Eglur.lock")
    target, root_lock = _registry_target(lock_text, f"{context}.Eglur.lock")
    manifest_text = source_files.get("Eglur.toml")
    if manifest_text is None:
        raise ValueError(f"{context} must contain Eglur.toml")
    root_manifest = _validate_root_manifest(
        manifest_text,
        target,
        f"{context}.Eglur.toml",
    )
    acquisition = invocation.registry_acquisitions[0]
    if (acquisition.package, acquisition.version) != (
        target.coordinate.package,
        target.coordinate.version,
    ):
        raise ValueError(
            f"{context} registry acquisition does not match its locked coordinate"
        )
    finish = acquisition.provider_steps[-1]
    if not isinstance(finish, suite_model.RegistryFinishStep):
        raise TypeError(f"{context} registry acquisition has no terminal finish")
    accounted_entries = _accounted_provider_entries(acquisition, context)
    provider_files = (
        _validated_provider_files(accounted_entries, context)
        if finish.outcome == "complete"
        else None
    )
    provider_tree_integrity = (
        _provider_tree_integrity(provider_files) if provider_files is not None else None
    )
    provider_package_metadata = (
        _provider_package_metadata(provider_files, context)
        if provider_files is not None
        and provider_tree_integrity == target.coordinate.integrity
        else None
    )
    return _FixtureFacts(
        coordinate=target.coordinate,
        dependency_key=target.dependency_key,
        target_package_name=target.package_name,
        target_node=target.registry_node,
        provider_package_metadata=provider_package_metadata,
        provider_tree_integrity=provider_tree_integrity,
        provider_finish=finish.outcome,
        outcome=test.expectation.outcome,
        input_context=_FixtureInputContext(
            root_manifest=root_manifest,
            root_lock=root_lock,
            source_files=tuple(
                item
                for item in invocation.source.files
                if item.path not in {"Eglur.toml", "Eglur.lock"}
            ),
            symlinks=invocation.source.symlinks,
            entry=invocation.source.entry,
            root=invocation.source.root,
            mode=invocation.mode,
            runner=invocation.runner,
            provider_steps=acquisition.provider_steps,
        ),
    )


def _references(profile: model.RegistryCacheProfile) -> set[suite_model.TestIdentity]:
    references: set[suite_model.TestIdentity] = set()
    for case in profile.cases:
        if isinstance(case, model.CoordinateIsolationCase):
            references.add(case.priming_fixture)
            references.add(case.package_fixture)
            references.add(case.version_fixture)
            references.add(case.integrity_fixture)
        else:
            references.add(case.fixture)
    return references


def _load_fixture_facts(
    profile: model.RegistryCacheProfile,
    bundle: suite_model.ConformanceBundle,
) -> tuple[
    dict[suite_model.TestIdentity, _FixtureFacts],
    list[ValidationProblem],
]:
    facts: dict[suite_model.TestIdentity, _FixtureFacts] = {}
    problems: list[ValidationProblem] = []
    for reference in sorted(_references(profile)):
        suite = bundle.find_suite(reference.suite_id)
        if suite is None:
            problems.append(
                ValidationProblem(
                    f"unknown registry-cache fixture suite {reference.suite_id}"
                )
            )
            continue
        if not isinstance(suite.entry.participation, suite_model.BaselineParticipation):
            problems.append(
                ValidationProblem(
                    f"registry-cache fixture suite {reference.suite_id} "
                    "must be unconditionally required"
                )
            )
            continue
        bound_test = bundle.find_test(reference)
        if bound_test is None:
            problems.append(
                ValidationProblem(f"unknown registry-cache fixture {reference}")
            )
            continue
        try:
            facts[reference] = _fixture_facts(reference, bound_test.case)
        except (TypeError, ValueError) as error:
            problems.append(ValidationProblem(str(error)))
    return facts, problems


def _success_problems(
    case_id: str,
    facts: _FixtureFacts,
) -> list[ValidationProblem]:
    problems: list[ValidationProblem] = []
    if facts.provider_finish != "complete":
        problems.append(
            ValidationProblem(
                f"{case_id} requires a fixture with complete provider acquisition"
            )
        )
    if not isinstance(facts.outcome, suite_model.ValueOutcome):
        problems.append(
            ValidationProblem(
                f"{case_id} requires a fixture with a successful value outcome"
            )
        )
    expected_metadata = (facts.target_package_name, facts.coordinate.version)
    if facts.provider_package_metadata != expected_metadata:
        problems.append(
            ValidationProblem(
                f"{case_id} successful fixture registry manifest must match its lock"
            )
        )
    if facts.provider_tree_integrity != facts.coordinate.integrity:
        problems.append(
            ValidationProblem(
                f"{case_id} successful fixture provider tree must match its lock"
            )
        )
    return problems


def _provider_failure_code(
    finish: suite_model.RegistryProviderFinish,
) -> str | None:
    if finish == "missing_release":
        return "PACKAGE_REGISTRY_RELEASE_MISSING"
    if finish == "transport_failure":
        return "PACKAGE_REGISTRY_TRANSPORT_FAILED"
    return None


def _acquisition_failure_problems(
    case_id: str,
    facts: _FixtureFacts,
) -> list[ValidationProblem]:
    expected_code = _provider_failure_code(facts.provider_finish)
    if expected_code is None or facts.compile_error_code != expected_code:
        return [
            ValidationProblem(
                f"{case_id} requires a provider failure and matching "
                "registry acquisition diagnostic"
            )
        ]
    return _diagnostic_coordinate_problems(case_id, facts)


def _diagnostic_coordinate_problems(
    case_id: str,
    facts: _FixtureFacts,
) -> list[ValidationProblem]:
    data = facts.compile_error_data_equals
    problems: list[ValidationProblem] = []
    if data.get("package") != facts.coordinate.package:
        problems.append(
            ValidationProblem(
                f"{case_id} diagnostic package must match the locked coordinate"
            )
        )
    if data.get("version") != facts.coordinate.version:
        problems.append(
            ValidationProblem(
                f"{case_id} diagnostic version must match the locked coordinate"
            )
        )
    return problems


def _target_package_context_problems(
    case_id: str,
    facts: _FixtureFacts,
) -> list[ValidationProblem]:
    data = facts.compile_error_data_equals
    expected = {
        "package_role": "dependency",
        "dependency": facts.dependency_key,
        "source_kind": "registry",
        "registry_package": facts.coordinate.package,
    }
    return [
        ValidationProblem(
            f"{case_id} target-package diagnostic {key} must be {value!r}"
        )
        for key, value in expected.items()
        if data.get(key) != value
    ]


def _validate_single_fixture_case(
    case: model.SingleFixtureCase,
    facts: _FixtureFacts,
) -> list[ValidationProblem]:
    if case.scenario in {
        "cold_then_warm",
        "cache_lookup_unavailable_fallback",
        "cache_publication_unavailable_fallback",
        "recovered_incomplete_is_miss",
    }:
        return _success_problems(case.case_id, facts)
    if case.scenario in {
        "failed_acquisition_not_published",
        "cache_lookup_unavailable_preserves_provider_failure",
    }:
        return _acquisition_failure_problems(case.case_id, facts)
    if facts.provider_finish != "complete":
        return [
            ValidationProblem(
                f"{case.case_id} requires a complete registry acquisition"
            )
        ]
    if facts.compile_error_code != "PACKAGE_MANIFEST_MISSING":
        return [
            ValidationProblem(
                f"{case.case_id} requires a missing target package manifest fixture"
            )
        ]
    problems = _target_package_context_problems(case.case_id, facts)
    if facts.provider_package_metadata is not None:
        problems.append(
            ValidationProblem(
                f"{case.case_id} invalid-package fixture must omit registry Eglur.toml"
            )
        )
    if facts.provider_tree_integrity != facts.coordinate.integrity:
        problems.append(
            ValidationProblem(
                f"{case.case_id} invalid-package fixture provider tree must match its lock"
            )
        )
    return problems


def _coordinate_relation_problems(
    case_id: str,
    label: str,
    priming: _FixtureFacts,
    variant: _FixtureFacts,
    changed_component: str,
) -> list[ValidationProblem]:
    problems: list[ValidationProblem] = []
    for component in ("package", "version", "integrity"):
        priming_value = getattr(priming.coordinate, component)
        variant_value = getattr(variant.coordinate, component)
        if component == changed_component and priming_value == variant_value:
            problems.append(
                ValidationProblem(
                    f"{case_id} {label} fixture must change only {changed_component}"
                )
            )
        if component != changed_component and priming_value != variant_value:
            problems.append(
                ValidationProblem(
                    f"{case_id} {label} fixture must preserve {component}"
                )
            )
    if variant.input_context != priming.input_context:
        problems.append(
            ValidationProblem(
                f"{case_id} {label} fixture must preserve non-coordinate "
                "invocation inputs"
            )
        )
    return problems


def _validate_coordinate_isolation_case(
    case: model.CoordinateIsolationCase,
    priming: _FixtureFacts,
    package_variant: _FixtureFacts,
    version_variant: _FixtureFacts,
    integrity_variant: _FixtureFacts,
) -> list[ValidationProblem]:
    problems = _success_problems(case.case_id, priming)
    problems.extend(_success_problems(case.case_id, package_variant))
    problems.extend(
        _coordinate_relation_problems(
            case.case_id,
            "package",
            priming,
            package_variant,
            "package",
        )
    )
    if package_variant.provider_tree_integrity != priming.provider_tree_integrity:
        problems.append(
            ValidationProblem(
                f"{case.case_id} package fixture must acquire the priming package tree"
            )
        )

    if version_variant.provider_finish != "complete":
        problems.append(
            ValidationProblem(
                f"{case.case_id} version fixture requires complete provider acquisition"
            )
        )
    if version_variant.compile_error_code != "PACKAGE_LOCK_OUT_OF_SYNC":
        problems.append(
            ValidationProblem(
                f"{case.case_id} version fixture must assert PACKAGE_LOCK_OUT_OF_SYNC"
            )
        )
    version_data = version_variant.compile_error_data_equals
    if version_data.get("reason") != "package_metadata":
        problems.append(
            ValidationProblem(
                f"{case.case_id} version fixture must assert package_metadata"
            )
        )
    if version_data.get("node") != version_variant.target_node:
        problems.append(
            ValidationProblem(
                f"{case.case_id} version fixture node must identify its registry target"
            )
        )
    if version_variant.provider_tree_integrity != priming.provider_tree_integrity:
        problems.append(
            ValidationProblem(
                f"{case.case_id} version fixture must acquire the priming package tree"
            )
        )
    problems.extend(
        _coordinate_relation_problems(
            case.case_id,
            "version",
            priming,
            version_variant,
            "version",
        )
    )

    if integrity_variant.provider_finish != "complete":
        problems.append(
            ValidationProblem(
                f"{case.case_id} integrity fixture requires complete provider acquisition"
            )
        )
    if integrity_variant.compile_error_code != "PACKAGE_REGISTRY_INTEGRITY_MISMATCH":
        problems.append(
            ValidationProblem(
                f"{case.case_id} integrity fixture must assert "
                "PACKAGE_REGISTRY_INTEGRITY_MISMATCH"
            )
        )
    problems.extend(_diagnostic_coordinate_problems(case.case_id, integrity_variant))
    integrity_data = integrity_variant.compile_error_data_equals
    if integrity_data.get("expected") != integrity_variant.coordinate.integrity:
        problems.append(
            ValidationProblem(
                f"{case.case_id} integrity fixture expected digest must match its lock"
            )
        )
    if integrity_data.get("actual") != priming.coordinate.integrity:
        problems.append(
            ValidationProblem(
                f"{case.case_id} integrity fixture actual digest must match the "
                "priming snapshot"
            )
        )
    if integrity_variant.provider_tree_integrity != priming.provider_tree_integrity:
        problems.append(
            ValidationProblem(
                f"{case.case_id} integrity fixture must acquire the priming package tree"
            )
        )
    problems.extend(
        _coordinate_relation_problems(
            case.case_id,
            "integrity",
            priming,
            integrity_variant,
            "integrity",
        )
    )
    return problems


def validate_profile(
    profile: model.RegistryCacheProfile,
    bundle: suite_model.ConformanceBundle,
) -> list[ValidationProblem]:
    """Validate profile cases against one canonical source-fixture bundle.

    Parameters
    ----------
    profile : RegistryCacheProfile
        Decoded cache probe definition.
    bundle : ConformanceBundle
        Manifest and exact suite documents referenced by the profile.

    Returns
    -------
    list[ValidationProblem]
        Deterministically ordered semantic validation failures.
    """
    facts, problems = _load_fixture_facts(profile, bundle)
    for case in profile.cases:
        if isinstance(case, model.SingleFixtureCase):
            fixture_facts = facts.get(case.fixture)
            if fixture_facts is not None:
                problems.extend(_validate_single_fixture_case(case, fixture_facts))
            continue
        if isinstance(case, model.CoordinateIsolationCase):
            priming = facts.get(case.priming_fixture)
            package_variant = facts.get(case.package_fixture)
            version_variant = facts.get(case.version_fixture)
            integrity_variant = facts.get(case.integrity_fixture)
            if (
                priming is not None
                and package_variant is not None
                and version_variant is not None
                and integrity_variant is not None
            ):
                problems.extend(
                    _validate_coordinate_isolation_case(
                        case,
                        priming,
                        package_variant,
                        version_variant,
                        integrity_variant,
                    )
                )
            continue
        fixture_facts = facts.get(case.fixture)
        if fixture_facts is not None:
            problems.extend(_success_problems(case.case_id, fixture_facts))
    return problems

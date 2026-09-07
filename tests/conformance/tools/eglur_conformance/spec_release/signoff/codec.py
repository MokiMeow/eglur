"""Decode the closed specification-release sign-off YAML schema."""

from datetime import datetime
from typing import cast

from eglur_conformance import release_identity
from eglur_conformance.artifact_content import expect_digest
from eglur_conformance.spec_release.signoff import model
from eglur_conformance.yaml_document import YamlValue

SPECIFICATION_KEYS = {"release", "commit"}
SPEC_RELEASE_SIGNOFF_KEYS = {
    "kind",
    "specification",
    "coverage_scope",
    "artifact_inventory",
    "manifest",
    "conformance_bundle_digest",
    "traceability",
    "open_items_ledger",
    "suite_definitions",
    "hard_required_gap_count",
    "conditional_required_asserted_count",
    "optional_suites",
    "optional_probe_optional_emit_if_present_asserted_count",
    "optional_probe_warning_active",
    "optional_probe_warning_note",
    "optional_probe_warning_threshold_min_asserted",
    "open_items_unclassified_count",
    "open_items_baseline_must_close_open_count",
    "implementation_conformance",
    "verdict",
    "approver",
    "timestamp_utc",
}


def _expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def _expect_str(value: YamlValue, context: str) -> str:
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if value == "":
        raise ValueError(f"{context} must be non-empty")
    return value


def _expect_concrete_str(value: YamlValue, context: str) -> str:
    narrowed = _expect_str(value, context)
    stripped = narrowed.strip()
    if stripped in {"TODO", "TBD"} or (
        stripped.startswith("<") and stripped.endswith(">")
    ):
        raise ValueError(f"{context} must be concrete")
    return narrowed


def _expect_int(value: YamlValue, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{context} must be an integer")
    return value


def _expect_bool(value: YamlValue, context: str) -> bool:
    if not isinstance(value, bool):
        raise TypeError(f"{context} must be a boolean")
    return value


def _expect_exact_keys(
    mapping: dict[str, YamlValue],
    expected: set[str],
    context: str,
) -> None:
    missing = sorted(expected - set(mapping))
    unknown = sorted(set(mapping) - expected)
    if missing or unknown:
        details: list[str] = []
        if missing:
            details.append(f"missing {missing}")
        if unknown:
            details.append(f"unsupported {unknown}")
        raise ValueError(f"{context} has invalid keys: {', '.join(details)}")


def decode_specification(
    value: YamlValue,
    context: str,
) -> model.SpecificationIdentity:
    """Decode a closed specification release identity."""
    specification = _expect_mapping(value, context)
    _expect_exact_keys(specification, SPECIFICATION_KEYS, context)
    release = _expect_str(specification.get("release"), f"{context}.release")
    release_identity.parse_release_label(release, f"{context}.release")
    return model.SpecificationIdentity(
        release=release,
        commit=_expect_str(specification.get("commit"), f"{context}.commit"),
    )


def parse_fraction(value: YamlValue, context: str) -> model.AssertionCount:
    """Parse an integer count or canonical asserted/total count."""
    if isinstance(value, bool):
        raise TypeError(f"{context} must be an integer or '<n>/<n>' string")
    if isinstance(value, int):
        return model.AssertionCount(asserted=value, total=value)
    if isinstance(value, str):
        parts = value.split("/")
        if len(parts) != 2:
            raise ValueError(f"{context} must use '<n>/<n>' format")
        try:
            return model.AssertionCount(
                asserted=int(parts[0]),
                total=int(parts[1]),
            )
        except ValueError as error:
            raise ValueError(
                f"{context} must contain integer fraction parts"
            ) from error
    raise TypeError(f"{context} must be an integer or '<n>/<n>' string")


def _timestamp_text(value: YamlValue, context: str) -> str:
    if isinstance(value, datetime):
        return value.isoformat()
    return _expect_str(value, context)


def decode_spec_package_signoff(
    document: YamlValue,
    document_context: str = "spec release sign-off",
) -> model.SpecPackageSignoff:
    """Decode and narrow one closed spec-package sign-off document."""
    root = _expect_mapping(document, document_context)
    _expect_exact_keys(root, SPEC_RELEASE_SIGNOFF_KEYS, document_context)
    context = "signoff"

    return model.SpecPackageSignoff(
        kind=_expect_str(root.get("kind"), f"{context}.kind"),
        specification=decode_specification(
            root.get("specification"),
            f"{context}.specification",
        ),
        conformance_bundle_digest=expect_digest(
            root.get("conformance_bundle_digest"),
            f"{context}.conformance_bundle_digest",
        ),
        traceability_snapshot=model.TraceabilitySnapshot(
            coverage_scope=_expect_str(
                root.get("coverage_scope"),
                f"{context}.coverage_scope",
            ),
            artifact_paths=model.ArtifactPaths(
                artifact_inventory=_expect_str(
                    root.get("artifact_inventory"),
                    f"{context}.artifact_inventory",
                ),
                manifest=_expect_str(
                    root.get("manifest"),
                    f"{context}.manifest",
                ),
                traceability=_expect_str(
                    root.get("traceability"),
                    f"{context}.traceability",
                ),
                open_items_ledger=_expect_str(
                    root.get("open_items_ledger"),
                    f"{context}.open_items_ledger",
                ),
            ),
            hard_required_gap_count=_expect_int(
                root.get("hard_required_gap_count"),
                f"{context}.hard_required_gap_count",
            ),
            conditional_required_asserted_count=parse_fraction(
                root.get("conditional_required_asserted_count"),
                f"{context}.conditional_required_asserted_count",
            ),
            optional_suites=_expect_str(
                root.get("optional_suites"),
                f"{context}.optional_suites",
            ),
            optional_probe_optional_emit_if_present_asserted_count=_expect_int(
                root.get("optional_probe_optional_emit_if_present_asserted_count"),
                (f"{context}.optional_probe_optional_emit_if_present_asserted_count"),
            ),
            optional_probe_warning_active=_expect_bool(
                root.get("optional_probe_warning_active"),
                f"{context}.optional_probe_warning_active",
            ),
            optional_probe_warning_note=_expect_str(
                root.get("optional_probe_warning_note"),
                f"{context}.optional_probe_warning_note",
            ),
            optional_probe_warning_threshold_min_asserted=_expect_int(
                root.get("optional_probe_warning_threshold_min_asserted"),
                f"{context}.optional_probe_warning_threshold_min_asserted",
            ),
            open_items_unclassified_count=_expect_int(
                root.get("open_items_unclassified_count"),
                f"{context}.open_items_unclassified_count",
            ),
            open_items_baseline_must_close_open_count=_expect_int(
                root.get("open_items_baseline_must_close_open_count"),
                f"{context}.open_items_baseline_must_close_open_count",
            ),
        ),
        suite_definitions=_expect_str(
            root.get("suite_definitions"),
            f"{context}.suite_definitions",
        ),
        implementation_conformance=_expect_str(
            root.get("implementation_conformance"),
            f"{context}.implementation_conformance",
        ),
        verdict=_expect_str(root.get("verdict"), f"{context}.verdict"),
        approval=model.Approval(
            approver=_expect_concrete_str(
                root.get("approver"),
                f"{context}.approver",
            ),
            timestamp_utc=_timestamp_text(
                root.get("timestamp_utc"),
                f"{context}.timestamp_utc",
            ),
        ),
    )

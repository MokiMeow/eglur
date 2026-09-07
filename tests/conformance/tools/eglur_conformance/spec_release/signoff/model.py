"""Canonical specification-release sign-off values."""

from dataclasses import dataclass


@dataclass(frozen=True)
class SpecificationIdentity:
    """One immutable Eglur specification release identity."""

    release: str
    commit: str


@dataclass(frozen=True)
class ArtifactPaths:
    """Canonical repository paths bound by a release sign-off."""

    artifact_inventory: str
    manifest: str
    traceability: str
    open_items_ledger: str


@dataclass(frozen=True)
class AssertionCount:
    """An asserted count paired with its complete expected population."""

    asserted: int
    total: int


@dataclass(frozen=True)
class TraceabilitySnapshot:
    """Traceability and OPEN-ledger facts copied into a sign-off."""

    coverage_scope: str
    artifact_paths: ArtifactPaths
    hard_required_gap_count: int
    conditional_required_asserted_count: AssertionCount
    optional_suites: str
    optional_probe_optional_emit_if_present_asserted_count: int
    optional_probe_warning_active: bool
    optional_probe_warning_note: str
    optional_probe_warning_threshold_min_asserted: int
    open_items_unclassified_count: int
    open_items_baseline_must_close_open_count: int


@dataclass(frozen=True)
class Approval:
    """Human approval identity and UTC timestamp."""

    approver: str
    timestamp_utc: str


@dataclass(frozen=True)
class SpecPackageSignoff:
    """Closed and typed specification-package release sign-off."""

    kind: str
    specification: SpecificationIdentity
    conformance_bundle_digest: str
    traceability_snapshot: TraceabilitySnapshot
    suite_definitions: str
    implementation_conformance: str
    verdict: str
    approval: Approval


@dataclass(frozen=True)
class SignoffExpectations:
    """Release facts against which one sign-off is validated."""

    release: str | None
    candidate_commit: str
    artifact_paths: ArtifactPaths
    coverage_scope: str
    conformance_bundle_digest: str
    hard_required_gap_count: int
    conditional_required_asserted_count: AssertionCount
    optional_probe_optional_emit_if_present_asserted_count: int
    optional_probe_warning_active: bool
    optional_probe_warning_threshold_min_asserted: int
    open_items_unclassified_count: int
    open_items_baseline_must_close_open_count: int

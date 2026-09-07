"""Validate typed specification-release sign-offs against canonical facts."""

from datetime import datetime

from eglur_conformance.spec_release.signoff import model
from eglur_conformance.validation import ValidationProblem


def validate_timestamp(value: str, context: str) -> list[ValidationProblem]:
    """Require an ISO-8601 timestamp with an explicit zero UTC offset."""
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError:
        return [ValidationProblem(f"{context} must be an ISO-8601 timestamp")]
    offset = parsed.utcoffset()
    if parsed.tzinfo is None or offset is None:
        return [ValidationProblem(f"{context} must include a UTC offset")]
    if offset.total_seconds() != 0:
        return [ValidationProblem(f"{context} must be UTC")]
    return []


def validate(
    signoff: model.SpecPackageSignoff,
    expected: model.SignoffExpectations,
) -> list[ValidationProblem]:
    """Return all sign-off mismatches in stable release-gate order."""
    problems: list[ValidationProblem] = []

    if (
        expected.release is not None
        and signoff.specification.release != expected.release
    ):
        problems.append(
            ValidationProblem(
                "signoff.specification.release is "
                f"{signoff.specification.release}, expected "
                f"{expected.release}"
            )
        )
    if signoff.specification.commit.lower() != expected.candidate_commit.lower():
        problems.append(
            ValidationProblem(
                "signoff.specification.commit is "
                f"{signoff.specification.commit}, expected "
                f"{expected.candidate_commit}"
            )
        )

    actual_snapshot = signoff.traceability_snapshot
    actual_paths = actual_snapshot.artifact_paths
    expected_paths = expected.artifact_paths
    path_values = (
        (
            "artifact_inventory",
            actual_paths.artifact_inventory,
            expected_paths.artifact_inventory,
        ),
        ("manifest", actual_paths.manifest, expected_paths.manifest),
        (
            "traceability",
            actual_paths.traceability,
            expected_paths.traceability,
        ),
        (
            "open_items_ledger",
            actual_paths.open_items_ledger,
            expected_paths.open_items_ledger,
        ),
    )
    for name, actual, expected_path in path_values:
        if actual != expected_path:
            problems.append(
                ValidationProblem(
                    f"signoff.{name} is {actual}, expected {expected_path}"
                )
            )

    if actual_snapshot.coverage_scope != expected.coverage_scope:
        problems.append(
            ValidationProblem(
                "signoff.coverage_scope is "
                f"{actual_snapshot.coverage_scope}, expected "
                f"{expected.coverage_scope}"
            )
        )

    integer_values = (
        (
            "hard_required_gap_count",
            actual_snapshot.hard_required_gap_count,
            expected.hard_required_gap_count,
        ),
        (
            "optional_probe_optional_emit_if_present_asserted_count",
            (actual_snapshot.optional_probe_optional_emit_if_present_asserted_count),
            expected.optional_probe_optional_emit_if_present_asserted_count,
        ),
        (
            "optional_probe_warning_threshold_min_asserted",
            actual_snapshot.optional_probe_warning_threshold_min_asserted,
            expected.optional_probe_warning_threshold_min_asserted,
        ),
        (
            "open_items_unclassified_count",
            actual_snapshot.open_items_unclassified_count,
            expected.open_items_unclassified_count,
        ),
        (
            "open_items_baseline_must_close_open_count",
            actual_snapshot.open_items_baseline_must_close_open_count,
            expected.open_items_baseline_must_close_open_count,
        ),
    )
    for name, actual, expected_count in integer_values:
        if actual != expected_count:
            problems.append(
                ValidationProblem(
                    f"signoff.{name} is {actual}, expected {expected_count}"
                )
            )

    actual_conditional = actual_snapshot.conditional_required_asserted_count
    expected_conditional = expected.conditional_required_asserted_count
    if actual_conditional != expected_conditional:
        problems.append(
            ValidationProblem(
                "signoff.conditional_required_asserted_count is "
                f"{actual_conditional.asserted}/{actual_conditional.total}, expected "
                f"{expected_conditional.asserted}/{expected_conditional.total}"
            )
        )

    if actual_snapshot.optional_suites != "non-blocking":
        problems.append(
            ValidationProblem(
                "signoff.optional_suites is "
                f"{actual_snapshot.optional_suites}, expected non-blocking"
            )
        )

    if (
        actual_snapshot.optional_probe_warning_active
        != expected.optional_probe_warning_active
    ):
        problems.append(
            ValidationProblem(
                "signoff.optional_probe_warning_active is "
                f"{actual_snapshot.optional_probe_warning_active}, expected "
                f"{expected.optional_probe_warning_active}"
            )
        )
    if (
        actual_snapshot.optional_probe_warning_active
        and actual_snapshot.optional_probe_warning_note.strip()
        in {
            "",
            "TODO",
            "<warning-text-or-n/a>",
            "n/a",
            "N/A",
        }
    ):
        problems.append(
            ValidationProblem(
                "signoff.optional_probe_warning_note must record the active warning"
            )
        )

    problems.extend(
        validate_timestamp(
            signoff.approval.timestamp_utc,
            "signoff.timestamp_utc",
        )
    )

    if signoff.conformance_bundle_digest != expected.conformance_bundle_digest:
        problems.append(
            ValidationProblem(
                "signoff.conformance_bundle_digest is "
                f"{signoff.conformance_bundle_digest}, expected "
                f"{expected.conformance_bundle_digest}"
            )
        )

    fixed_values = (
        ("kind", signoff.kind, "spec_package_release"),
        ("suite_definitions", signoff.suite_definitions, "VALID"),
        (
            "implementation_conformance",
            signoff.implementation_conformance,
            "not_evaluated",
        ),
        ("verdict", signoff.verdict, "GO"),
    )
    for name, actual, expected_value in fixed_values:
        if actual != expected_value:
            problems.append(
                ValidationProblem(
                    f"signoff.{name} is {actual}, expected {expected_value}"
                )
            )
    return problems

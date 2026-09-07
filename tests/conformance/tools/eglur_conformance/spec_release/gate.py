"""Orchestrate specification-package release readiness validation."""

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import yaml

from eglur_conformance.effects import codec as effect_codec
from eglur_conformance.registry_cache import repository as registry_cache_repository
from eglur_conformance.spec_release import (
    artifact_inventory,
    git_repository,
    history,
    normative_keywords,
    open_items,
    release_state,
    repository,
)
from eglur_conformance.spec_release.signoff import codec as signoff_codec
from eglur_conformance.spec_release.signoff import model as signoff_model
from eglur_conformance.spec_release.signoff import policy as signoff_policy
from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.traceability import codec as traceability_codec
from eglur_conformance.traceability import model as traceability_model
from eglur_conformance.traceability import repository as traceability_repository
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import load_yaml

STATIC_CHECK_LABELS = (
    "pixi -q run --locked validate-artifacts",
    "pixi -q run --locked validate-schema",
    "pixi -q run --locked validate-effects",
    "pixi -q run --locked test",
    "pixi -q run --locked validate-traceability",
    "pixi -q run --locked validate-open-items",
    "pixi -q run --locked validate-keywords",
    str(repository.DEFAULT_TRACEABILITY_PATH),
)


@dataclass(frozen=True)
class GateEvaluation:
    """One complete specification-package readiness evaluation."""

    problems: tuple[ValidationProblem, ...]
    checked: tuple[str, ...]

    @property
    def ready(self) -> bool:
        """Return whether every required check passed."""
        return not self.problems


def validate_traceability_freshness(
    repo_root: Path,
) -> list[ValidationProblem]:
    """Validate byte-exact traceability freshness without rewriting the artifact."""
    traceability_path = repo_root / repository.DEFAULT_TRACEABILITY_PATH
    report = traceability_repository.build_report(repo_root)
    generated = traceability_codec.encode_report(report)
    current = traceability_path.read_bytes()
    problems: list[ValidationProblem] = []
    if current != generated:
        problems.append(
            ValidationProblem(
                f"{repository.DEFAULT_TRACEABILITY_PATH} is stale; run "
                "pixi -q run --locked generate-traceability"
            )
        )
    if report.contract_failures:
        problems.append(
            ValidationProblem(
                "diagnostic contract validation failed: "
                f"{len(report.contract_failures)} missing assertions"
            )
        )
    return problems


def validate_schema_validator_tests(repo_root: Path) -> list[ValidationProblem]:
    """Run schema-validator regression tests as part of the spec-package gate."""
    completed = subprocess.run(
        [
            sys.executable,
            "-m",
            "unittest",
            "discover",
            "-s",
            "tests/conformance/tools",
        ],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    output = "\n".join(
        part.strip() for part in (completed.stdout, completed.stderr) if part.strip()
    )
    if completed.returncode == 0:
        summary = re.search(r"\bRan ([0-9]+) tests?\b", output)
        if summary is None:
            return [
                ValidationProblem(
                    "schema-validator regression tests did not report a test count"
                )
            ]
        if int(summary.group(1)) == 0:
            return [
                ValidationProblem(
                    "schema-validator regression tests discovered zero tests"
                )
            ]
        return []

    return [
        ValidationProblem(
            "schema-validator regression tests failed"
            + (f":\n{output}" if output else "")
        )
    ]


def validate_effect_summary_catalog(repo_root: Path) -> list[ValidationProblem]:
    """Validate trusted stdlib effects against the indexed callable surface."""
    try:
        effect_codec.load_effect_catalog(repo_root)
    except (OSError, TypeError, ValueError, yaml.YAMLError) as error:
        return [ValidationProblem(f"effect-summary catalog invalid: {error}")]
    return []


def validate_clean_checkout(
    git: git_repository.GitRepository,
) -> list[ValidationProblem]:
    """Reject evidence evaluated against tracked changes outside spec HEAD."""
    if git.merge_head_commits():
        return [
            ValidationProblem(
                "evidence requires a completed Git history; MERGE_HEAD is present"
            )
        ]
    if git.tracked_checkout_differs_from_head():
        return [
            ValidationProblem(
                "evidence requires a clean tracked checkout of specification HEAD"
            )
        ]
    return []


def validate_open_item_readiness(
    ledger: open_items.Ledger,
) -> list[ValidationProblem]:
    """Return release-readiness problems from canonical OPEN-item counts.

    Parameters
    ----------
    ledger : open_items.Ledger
        Decoded ledger whose summary is evaluated after structural validation.

    Returns
    -------
    list[ValidationProblem]
        One problem for each summary count that prevents a release verdict.
    """
    problems: list[ValidationProblem] = []
    summary = ledger.summary
    if summary["unclassified_count"] != 0:
        problems.append(
            ValidationProblem(
                "open-items ledger summary.unclassified_count is "
                f"{summary['unclassified_count']}, expected 0"
            )
        )
    if summary["baseline_must_close_open_count"] != 0:
        problems.append(
            ValidationProblem(
                "open-items ledger summary.baseline_must_close_open_count is "
                f"{summary['baseline_must_close_open_count']}, expected 0"
            )
        )
    return problems


def validate_static(
    git: git_repository.GitRepository,
    snapshot: repository.SpecPackageSnapshot,
) -> list[ValidationProblem]:
    """Validate static spec-package gate inputs present in the repository."""
    repo_root = git.root
    problems: list[ValidationProblem] = []
    problems.extend(
        artifact_inventory.validate_checkout_binding(repo_root)
        + artifact_inventory.validate_inventory(
            snapshot.inventory,
            artifact_inventory.discover_authoritative_artifacts(repo_root),
        )
    )
    problems.extend(
        suite_schema.validate_all(
            repo_root, repo_root / repository.DEFAULT_MANIFEST_PATH
        )
    )
    problems.extend(
        registry_cache_repository.validate_repository(
            repo_root,
            repo_root / repository.DEFAULT_MANIFEST_PATH,
        )
    )
    problems.extend(validate_effect_summary_catalog(repo_root))
    problems.extend(validate_schema_validator_tests(repo_root))
    problems.extend(validate_traceability_freshness(repo_root))
    source_inspection = open_items.inspect_sources(
        repo_root,
        snapshot.ledger.sources,
    )
    problems.extend(open_items.validate_ledger(snapshot.ledger, source_inspection))
    problems.extend(
        ValidationProblem(violation.render())
        for violation in normative_keywords.find_violations(repo_root)
    )

    artifact_state, state_problems = release_state.agree(snapshot.release_carriers())
    problems.extend(state_problems)
    if artifact_state is not None:
        problems.extend(history.validate(git, artifact_state))

    summary = snapshot.traceability.summary
    hard_gap_count = summary.hard_required_gap_count
    if hard_gap_count != 0:
        problems.append(
            ValidationProblem(
                f"hard_required_gap_count is {hard_gap_count}, expected 0"
            )
        )

    contract_failure_count = summary.contract_failure_count
    if contract_failure_count != 0:
        problems.append(
            ValidationProblem(
                f"contract_failure_count is {contract_failure_count}, expected 0"
            )
        )

    conditional_asserted_count = summary.conditional_required_asserted_count
    conditional_code_count = summary.conditional_required_code_count
    if conditional_asserted_count != conditional_code_count:
        problems.append(
            ValidationProblem(
                "conditional_required_asserted_count is "
                f"{conditional_asserted_count}, expected {conditional_code_count}"
            )
        )

    optional_threshold = (
        snapshot.traceability.optional_probe_policy.warning_threshold_min_asserted
    )
    if (
        optional_threshold
        != traceability_model.OPTIONAL_PROBE_WARNING_THRESHOLD_MIN_ASSERTED
    ):
        problems.append(
            ValidationProblem(
                "optional_probe_summary.warning_threshold_min_asserted is "
                f"{optional_threshold}, expected "
                f"{traceability_model.OPTIONAL_PROBE_WARNING_THRESHOLD_MIN_ASSERTED}"
            )
        )

    if not snapshot.traceability.optional_probe_suites:
        problems.append(
            ValidationProblem(
                "optional_probe_summary.optional_suites must be non-empty"
            )
        )

    problems.extend(validate_open_item_readiness(snapshot.ledger))
    return problems


def _build_signoff_expectations(
    snapshot: repository.SpecPackageSnapshot,
    candidate_commit: str,
) -> signoff_model.SignoffExpectations:
    """Build typed sign-off expectations from canonical release artifacts."""
    artifact_state, _ = release_state.agree(snapshot.release_carriers())
    summary = snapshot.traceability.summary
    return signoff_model.SignoffExpectations(
        release=artifact_state.release.text if artifact_state is not None else None,
        candidate_commit=candidate_commit,
        artifact_paths=signoff_model.ArtifactPaths(
            artifact_inventory=str(repository.DEFAULT_INVENTORY_PATH),
            manifest=str(repository.DEFAULT_MANIFEST_PATH),
            traceability=str(repository.DEFAULT_TRACEABILITY_PATH),
            open_items_ledger=str(repository.DEFAULT_LEDGER_PATH),
        ),
        coverage_scope=traceability_model.COVERAGE_SCOPE,
        conformance_bundle_digest=snapshot.conformance_bundle_digest,
        hard_required_gap_count=summary.hard_required_gap_count,
        conditional_required_asserted_count=signoff_model.AssertionCount(
            asserted=summary.conditional_required_asserted_count,
            total=summary.conditional_required_code_count,
        ),
        optional_probe_optional_emit_if_present_asserted_count=(
            summary.optional_probe_optional_emit_if_present_asserted_count
        ),
        optional_probe_warning_active=(
            snapshot.traceability.optional_probe_warning_active
        ),
        optional_probe_warning_threshold_min_asserted=(
            snapshot.traceability.optional_probe_policy.warning_threshold_min_asserted
        ),
        open_items_unclassified_count=snapshot.ledger.summary["unclassified_count"],
        open_items_baseline_must_close_open_count=snapshot.ledger.summary[
            "baseline_must_close_open_count"
        ],
    )


def validate_signoff(
    snapshot: repository.SpecPackageSnapshot,
    signoff: signoff_model.SpecPackageSignoff,
    candidate_commit: str,
) -> list[ValidationProblem]:
    """Validate a typed sign-off against one candidate snapshot."""
    problems = release_state.validate(
        snapshot.release_carriers(),
        require_released=True,
    )
    problems.extend(
        signoff_policy.validate(
            signoff,
            _build_signoff_expectations(snapshot, candidate_commit),
        )
    )
    return problems


def evaluate_spec_package(
    repo_root: Path,
    signoff_path: Path,
    signoff_label: str,
) -> GateEvaluation:
    """Evaluate one complete specification-package release candidate."""
    git = git_repository.GitRepository(repo_root)
    candidate_commit = git.current_commit()
    problems = validate_clean_checkout(git)
    problems.extend(artifact_inventory.validate_checkout_binding(repo_root))
    if problems:
        return GateEvaluation(problems=tuple(problems), checked=())

    snapshot = repository.load_snapshot(repo_root)
    problems.extend(validate_static(git, snapshot))
    signoff = signoff_codec.decode_spec_package_signoff(load_yaml(signoff_path))
    problems.extend(validate_signoff(snapshot, signoff, candidate_commit))
    return GateEvaluation(
        problems=tuple(problems),
        checked=(*STATIC_CHECK_LABELS, signoff_label),
    )

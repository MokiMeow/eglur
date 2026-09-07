"""Canonical diagnostic traceability facts and derived projections."""

from dataclasses import dataclass
from typing import Literal, TypeAlias

from eglur_conformance import release_identity
from eglur_conformance.diagnostics import model as diagnostic_model

VERSION = "0.1"
COVERAGE_SCOPE = "diagnostic_codes"
OPTIONAL_PROBE_MODE = "non_blocking_warning"
OPTIONAL_PROBE_WARNING_THRESHOLD_MIN_ASSERTED = 1
OPTIONAL_EXTENSION_MODE = "non_blocking_evidence"
ARTIFACT_STATUSES = frozenset({"draft", "released"})

ArtifactStatus: TypeAlias = Literal["draft", "released"]
DiagnosticSeverity: TypeAlias = Literal["error", "warning", "note"]
EvidenceStrength: TypeAlias = Literal["asserted", "mentioned_only", "uncovered"]

ASSERTED: EvidenceStrength = "asserted"
MENTIONED_ONLY: EvidenceStrength = "mentioned_only"
UNCOVERED: EvidenceStrength = "uncovered"


@dataclass(frozen=True, order=True)
class SuiteReference:
    """One suite identity used by traceability evidence."""

    suite_id: str
    file: str
    precondition: str | None

    def __post_init__(self) -> None:
        """Reject incomplete suite references."""
        if not self.suite_id:
            raise ValueError("suite reference id must be non-empty")
        if not self.file:
            raise ValueError("suite reference file must be non-empty")
        if self.precondition == "":
            raise ValueError("suite reference precondition must be non-empty")


@dataclass(frozen=True)
class SuiteEvidence:
    """One suite reference and its directly contributing test IDs."""

    suite: SuiteReference
    test_ids: tuple[str, ...]

    def __post_init__(self) -> None:
        """Require non-empty, unique, canonical test IDs."""
        if not self.test_ids:
            raise ValueError("suite evidence test_ids must be non-empty")
        if any(not test_id for test_id in self.test_ids):
            raise ValueError("suite evidence test_ids must be non-empty")
        if self.test_ids != tuple(sorted(set(self.test_ids))):
            raise ValueError("suite evidence test_ids must be unique and sorted")


@dataclass(frozen=True)
class CoverageEvidence:
    """The strongest canonical coverage evidence for one suite partition."""

    asserted_in: tuple[SuiteEvidence, ...]
    mentioned_only_in: tuple[SuiteEvidence, ...]

    def __post_init__(self) -> None:
        """Require one canonical evidence strength and ordering."""
        if self.asserted_in and self.mentioned_only_in:
            raise ValueError(
                "coverage evidence cannot be asserted and mentioned-only together"
            )
        entries = self.asserted_in or self.mentioned_only_in
        suites = tuple(entry.suite for entry in entries)
        if len(set(suites)) != len(suites):
            raise ValueError("coverage evidence suite references must be unique")
        if suites != tuple(sorted(suites)):
            raise ValueError("coverage evidence suite references must be sorted")

    @property
    def strength(self) -> EvidenceStrength:
        """Return the coverage strength derived from evidence presence."""
        if self.asserted_in:
            return ASSERTED
        if self.mentioned_only_in:
            return MENTIONED_ONLY
        return UNCOVERED


EMPTY_COVERAGE = CoverageEvidence(asserted_in=(), mentioned_only_in=())


@dataclass(frozen=True)
class DiagnosticTrace:
    """All required and optional conformance evidence for one diagnostic."""

    code: str
    severity: DiagnosticSeverity
    obligation: diagnostic_model.DiagnosticCoverageObligation
    required_suite_evidence: CoverageEvidence
    optional_suite_evidence: CoverageEvidence

    def __post_init__(self) -> None:
        """Require policy/precondition consistency and observable optional evidence."""
        if not self.code:
            raise ValueError("diagnostic trace code must be non-empty")
        if self.severity not in {"error", "warning", "note"}:
            raise ValueError(f"unsupported diagnostic severity {self.severity!r}")

        is_conditional = self.obligation.policy == diagnostic_model.CONDITIONAL_REQUIRED
        if is_conditional != (self.obligation.precondition is not None):
            raise ValueError(
                f"{self.code} conditional policy and precondition must agree"
            )
        if self.obligation.precondition == "":
            raise ValueError(f"{self.code} precondition must be non-empty")

        optional_evidence_is_observable = self.obligation.policy in {
            diagnostic_model.OPTIONAL_EMIT_IF_PRESENT,
            diagnostic_model.EXCLUDED_NONBASELINE,
        }
        if (
            not optional_evidence_is_observable
            and self.optional_suite_evidence.strength != UNCOVERED
        ):
            raise ValueError(
                f"{self.code} optional evidence has no traceability projection"
            )


@dataclass(frozen=True)
class ContractFailure:
    """One field-scoped assertion missing from diagnostic evidence."""

    code: str
    suite_id: str
    file: str
    test_id: str
    field: str
    missing: str

    @property
    def ordering_key(self) -> tuple[str, str, str, str, str, str]:
        """Return the stable artifact ordering key."""
        return (
            self.code,
            self.file,
            self.test_id,
            self.field,
            self.missing,
            self.suite_id,
        )


@dataclass(frozen=True)
class TraceabilitySources:
    """The source artifacts from which one report was derived."""

    diagnostic_catalogs: tuple[str, ...]
    diagnostic_contract: str
    suites_manifest: str

    def __post_init__(self) -> None:
        """Require concrete, canonical provenance paths."""
        if not self.diagnostic_catalogs:
            raise ValueError("traceability diagnostic catalogs must be non-empty")
        if self.diagnostic_catalogs != tuple(sorted(set(self.diagnostic_catalogs))):
            raise ValueError(
                "traceability diagnostic catalogs must be unique and sorted"
            )
        if not self.diagnostic_contract:
            raise ValueError("traceability diagnostic contract must be non-empty")
        if not self.suites_manifest:
            raise ValueError("traceability suites manifest must be non-empty")


@dataclass(frozen=True)
class OptionalProbePolicy:
    """Policy that turns optional diagnostic evidence into a warning state."""

    warning_threshold_min_asserted: int

    def __post_init__(self) -> None:
        """Require a non-negative warning threshold."""
        if self.warning_threshold_min_asserted < 0:
            raise ValueError("optional probe warning threshold must be non-negative")


@dataclass(frozen=True)
class ConditionalCode:
    """One conditional diagnostic and its activation precondition."""

    code: str
    precondition: str


@dataclass(frozen=True)
class TraceabilitySummary:
    """All count projections serialized in the traceability artifact."""

    diagnostic_code_count: int
    baseline_scope_code_count: int
    asserted_code_count: int
    mentioned_only_code_count: int
    uncovered_code_count: int
    hard_required_code_count: int
    hard_required_asserted_count: int
    hard_required_gap_count: int
    conditional_required_code_count: int
    conditional_required_asserted_count: int
    optional_emit_if_present_code_count: int
    optional_emit_if_present_asserted_count: int
    optional_probe_suite_count: int
    optional_probe_optional_emit_if_present_asserted_count: int
    optional_probe_optional_emit_if_present_mentioned_only_count: int
    optional_probe_optional_emit_if_present_unasserted_count: int
    optional_extension_code_count: int
    optional_extension_asserted_count: int
    optional_extension_mentioned_only_count: int
    optional_extension_unasserted_count: int
    optional_extension_suite_count: int
    excluded_nonbaseline_code_count: int
    contract_failure_count: int
    contract_message_token_missing_count: int
    contract_help_token_missing_count: int
    contract_related_token_missing_count: int
    contract_data_key_missing_count: int
    contract_data_equals_failure_count: int


@dataclass(frozen=True)
class TraceabilityReport:
    """Canonical trace facts from which every serialized projection is derived."""

    release_history_root: str
    release: release_identity.ReleaseLabel
    status: ArtifactStatus
    sources: TraceabilitySources
    diagnostics: tuple[DiagnosticTrace, ...]
    contract_failures: tuple[ContractFailure, ...]
    optional_probe_policy: OptionalProbePolicy

    def __post_init__(self) -> None:
        """Require unique, deterministic report facts."""
        if self.status not in ARTIFACT_STATUSES:
            raise ValueError(f"unsupported traceability status {self.status!r}")
        if not self.diagnostics:
            raise ValueError("traceability diagnostics must be non-empty")
        codes = tuple(trace.code for trace in self.diagnostics)
        if codes != tuple(sorted(set(codes))):
            raise ValueError("traceability diagnostic codes must be unique and sorted")
        expected_failures = tuple(
            sorted(self.contract_failures, key=lambda failure: failure.ordering_key)
        )
        if self.contract_failures != expected_failures:
            raise ValueError("traceability contract failures must be sorted")

    def _with_policy(
        self,
        policy: diagnostic_model.CoveragePolicy,
    ) -> tuple[DiagnosticTrace, ...]:
        """Return diagnostics owned by one coverage policy."""
        return tuple(
            trace for trace in self.diagnostics if trace.obligation.policy == policy
        )

    @property
    def conditional_required_codes(self) -> tuple[ConditionalCode, ...]:
        """Return conditional diagnostics and their activation preconditions."""
        result: list[ConditionalCode] = []
        for trace in self._with_policy(diagnostic_model.CONDITIONAL_REQUIRED):
            precondition = trace.obligation.precondition
            if precondition is None:
                raise AssertionError("conditional trace lacks a precondition")
            result.append(ConditionalCode(trace.code, precondition))
        return tuple(result)

    @property
    def excluded_nonbaseline_codes(self) -> tuple[str, ...]:
        """Return diagnostics excluded from the v0.1 baseline."""
        return tuple(
            trace.code
            for trace in self._with_policy(diagnostic_model.EXCLUDED_NONBASELINE)
        )

    @property
    def uncovered_codes(self) -> tuple[str, ...]:
        """Return diagnostics with no required-suite evidence."""
        return tuple(
            trace.code
            for trace in self.diagnostics
            if trace.required_suite_evidence.strength == UNCOVERED
        )

    @property
    def hard_gaps(self) -> tuple[str, ...]:
        """Return hard-required diagnostics without direct assertions."""
        return tuple(
            trace.code
            for trace in self._with_policy(diagnostic_model.HARD_REQUIRED_ASSERTED)
            if trace.required_suite_evidence.strength != ASSERTED
        )

    @property
    def conditional_pending(self) -> tuple[ConditionalCode, ...]:
        """Return unasserted conditional diagnostics and their preconditions."""
        pending: list[ConditionalCode] = []
        for trace in self._with_policy(diagnostic_model.CONDITIONAL_REQUIRED):
            if trace.required_suite_evidence.strength == ASSERTED:
                continue
            precondition = trace.obligation.precondition
            if precondition is None:
                raise AssertionError("conditional trace lacks a precondition")
            pending.append(ConditionalCode(trace.code, precondition))
        return tuple(pending)

    @property
    def optional_emit_unasserted(self) -> tuple[str, ...]:
        """Return optional-emission diagnostics not asserted by required suites."""
        return tuple(
            trace.code
            for trace in self._with_policy(diagnostic_model.OPTIONAL_EMIT_IF_PRESENT)
            if trace.required_suite_evidence.strength != ASSERTED
        )

    def _optional_suites(
        self,
        policy: diagnostic_model.CoveragePolicy,
    ) -> tuple[SuiteReference, ...]:
        """Return the sorted suite union for one optional evidence partition."""
        suites = {
            evidence.suite
            for trace in self._with_policy(policy)
            for evidence in (
                trace.optional_suite_evidence.asserted_in
                or trace.optional_suite_evidence.mentioned_only_in
            )
        }
        return tuple(sorted(suites))

    @property
    def optional_probe_suites(self) -> tuple[SuiteReference, ...]:
        """Return suites covering optional emit-if-present diagnostics."""
        return self._optional_suites(diagnostic_model.OPTIONAL_EMIT_IF_PRESENT)

    @property
    def optional_extension_suites(self) -> tuple[SuiteReference, ...]:
        """Return suites covering excluded non-baseline diagnostics."""
        return self._optional_suites(diagnostic_model.EXCLUDED_NONBASELINE)

    @property
    def optional_probe_warning_active(self) -> bool:
        """Return the persisted warning state derived from report evidence."""
        return (
            self.summary.optional_probe_optional_emit_if_present_asserted_count
            < self.optional_probe_policy.warning_threshold_min_asserted
        )

    @property
    def optional_probe_warning_message(self) -> str:
        """Return the canonical warning explanation for current probe evidence."""
        if self.optional_probe_warning_active:
            return (
                "optional emit-if-present probes have no asserted coverage; "
                "baseline gate remains non-blocking"
            )
        return "optional emit-if-present probes meet warning threshold"

    @property
    def summary(self) -> TraceabilitySummary:
        """Compute all artifact counts from canonical trace facts."""
        asserted = tuple(
            trace
            for trace in self.diagnostics
            if trace.required_suite_evidence.strength == ASSERTED
        )
        mentioned_only = tuple(
            trace
            for trace in self.diagnostics
            if trace.required_suite_evidence.strength == MENTIONED_ONLY
        )
        baseline = tuple(
            trace
            for trace in self.diagnostics
            if trace.obligation.policy != diagnostic_model.EXCLUDED_NONBASELINE
        )
        hard = self._with_policy(diagnostic_model.HARD_REQUIRED_ASSERTED)
        conditional = self._with_policy(diagnostic_model.CONDITIONAL_REQUIRED)
        optional_emit = self._with_policy(diagnostic_model.OPTIONAL_EMIT_IF_PRESENT)
        excluded = self._with_policy(diagnostic_model.EXCLUDED_NONBASELINE)

        def count_required(
            traces: tuple[DiagnosticTrace, ...],
            strength: EvidenceStrength,
        ) -> int:
            return sum(
                trace.required_suite_evidence.strength == strength for trace in traces
            )

        def count_optional(
            traces: tuple[DiagnosticTrace, ...],
            strength: EvidenceStrength,
        ) -> int:
            return sum(
                trace.optional_suite_evidence.strength == strength for trace in traces
            )

        return TraceabilitySummary(
            diagnostic_code_count=len(self.diagnostics),
            baseline_scope_code_count=len(baseline),
            asserted_code_count=len(asserted),
            mentioned_only_code_count=len(mentioned_only),
            uncovered_code_count=len(self.uncovered_codes),
            hard_required_code_count=len(hard),
            hard_required_asserted_count=count_required(hard, ASSERTED),
            hard_required_gap_count=len(self.hard_gaps),
            conditional_required_code_count=len(conditional),
            conditional_required_asserted_count=count_required(
                conditional,
                ASSERTED,
            ),
            optional_emit_if_present_code_count=len(optional_emit),
            optional_emit_if_present_asserted_count=count_required(
                optional_emit,
                ASSERTED,
            ),
            optional_probe_suite_count=len(self.optional_probe_suites),
            optional_probe_optional_emit_if_present_asserted_count=count_optional(
                optional_emit,
                ASSERTED,
            ),
            optional_probe_optional_emit_if_present_mentioned_only_count=count_optional(
                optional_emit,
                MENTIONED_ONLY,
            ),
            optional_probe_optional_emit_if_present_unasserted_count=count_optional(
                optional_emit,
                UNCOVERED,
            ),
            optional_extension_code_count=len(excluded),
            optional_extension_asserted_count=count_optional(excluded, ASSERTED),
            optional_extension_mentioned_only_count=count_optional(
                excluded,
                MENTIONED_ONLY,
            ),
            optional_extension_unasserted_count=count_optional(excluded, UNCOVERED),
            optional_extension_suite_count=len(self.optional_extension_suites),
            excluded_nonbaseline_code_count=len(excluded),
            contract_failure_count=len(self.contract_failures),
            contract_message_token_missing_count=sum(
                failure.field == "message_contains"
                for failure in self.contract_failures
            ),
            contract_help_token_missing_count=sum(
                failure.field == "help_contains" for failure in self.contract_failures
            ),
            contract_related_token_missing_count=sum(
                failure.field == "related_contains"
                for failure in self.contract_failures
            ),
            contract_data_key_missing_count=sum(
                failure.field == "data_keys" for failure in self.contract_failures
            ),
            contract_data_equals_failure_count=sum(
                failure.field == "data_equals" for failure in self.contract_failures
            ),
        )

"""Closed YAML decoding and exact byte encoding for traceability reports."""

import json
from dataclasses import dataclass
from typing import cast

from eglur_conformance import release_identity
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.traceability import model
from eglur_conformance.yaml_document import YamlValue

TRACEABILITY_KEYS = {
    "version",
    "release_history_root",
    "release",
    "status",
    "coverage_scope",
    "sources",
    "policy",
    "summary",
    "contract_validation",
    "coverage",
    "optional_probe_summary",
    "optional_probe_coverage",
    "optional_extension_summary",
    "optional_extension_coverage",
    "hard_gaps",
    "conditional_pending",
    "optional_emit_unasserted",
    "excluded_nonbaseline_codes",
    "uncovered_codes",
}
SOURCE_KEYS = {"required_codes", "diagnostic_contract", "suites_manifest"}
POLICY_KEYS = {"excluded_nonbaseline_codes", "conditional_required_codes"}
CONTRACT_VALIDATION_KEYS = {"failure_count", "failures"}
CONTRACT_FAILURE_KEYS = {
    "code",
    "suite_id",
    "file",
    "test_id",
    "field",
    "missing",
}
COVERAGE_REQUIRED_KEYS = {
    "code",
    "severity",
    "policy",
    "status",
    "asserted_in",
    "mentioned_only_in",
}
COVERAGE_OPTIONAL_KEYS = {"precondition"}
OPTIONAL_COVERAGE_KEYS = {
    "code",
    "status",
    "asserted_in",
    "mentioned_only_in",
}
EVIDENCE_REQUIRED_KEYS = {"id", "file", "test_ids"}
EVIDENCE_OPTIONAL_KEYS = {"precondition"}
OPTIONAL_PROBE_SUMMARY_KEYS = {
    "mode",
    "warning_threshold_min_asserted",
    "warning_active",
    "warning_message",
    "optional_suites",
}
OPTIONAL_EXTENSION_SUMMARY_KEYS = {"mode", "optional_suites"}
SUITE_REQUIRED_KEYS = {"id", "file"}
SUITE_OPTIONAL_KEYS = {"precondition"}
CONDITIONAL_KEYS = {"code", "precondition"}
SUMMARY_FIELDS = (
    ("required_code_count", "diagnostic_code_count"),
    ("baseline_scope_code_count", "baseline_scope_code_count"),
    ("asserted_code_count", "asserted_code_count"),
    ("mentioned_only_code_count", "mentioned_only_code_count"),
    ("uncovered_code_count", "uncovered_code_count"),
    ("hard_required_code_count", "hard_required_code_count"),
    ("hard_required_asserted_count", "hard_required_asserted_count"),
    ("hard_required_gap_count", "hard_required_gap_count"),
    ("conditional_required_code_count", "conditional_required_code_count"),
    (
        "conditional_required_asserted_count",
        "conditional_required_asserted_count",
    ),
    (
        "optional_emit_if_present_code_count",
        "optional_emit_if_present_code_count",
    ),
    (
        "optional_emit_if_present_asserted_count",
        "optional_emit_if_present_asserted_count",
    ),
    ("optional_probe_suite_count", "optional_probe_suite_count"),
    (
        "optional_probe_optional_emit_if_present_asserted_count",
        "optional_probe_optional_emit_if_present_asserted_count",
    ),
    (
        "optional_probe_optional_emit_if_present_mentioned_only_count",
        "optional_probe_optional_emit_if_present_mentioned_only_count",
    ),
    (
        "optional_probe_optional_emit_if_present_unasserted_count",
        "optional_probe_optional_emit_if_present_unasserted_count",
    ),
    ("optional_extension_code_count", "optional_extension_code_count"),
    ("optional_extension_asserted_count", "optional_extension_asserted_count"),
    (
        "optional_extension_mentioned_only_count",
        "optional_extension_mentioned_only_count",
    ),
    (
        "optional_extension_unasserted_count",
        "optional_extension_unasserted_count",
    ),
    ("optional_extension_suite_count", "optional_extension_suite_count"),
    (
        "excluded_nonbaseline_code_count",
        "excluded_nonbaseline_code_count",
    ),
    ("contract_failure_count", "contract_failure_count"),
    (
        "contract_message_token_missing_count",
        "contract_message_token_missing_count",
    ),
    ("contract_help_token_missing_count", "contract_help_token_missing_count"),
    (
        "contract_related_token_missing_count",
        "contract_related_token_missing_count",
    ),
    ("contract_data_key_missing_count", "contract_data_key_missing_count"),
    (
        "contract_data_equals_failure_count",
        "contract_data_equals_failure_count",
    ),
)
_SUMMARY_KEYS = {key for key, _attribute in SUMMARY_FIELDS}


@dataclass(frozen=True)
class _RequiredTrace:
    """One decoded required-suite trace before optional evidence is merged."""

    code: str
    severity: model.DiagnosticSeverity
    obligation: diagnostic_model.DiagnosticCoverageObligation
    evidence: model.CoverageEvidence


def _mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    """Narrow one YAML mapping to string keys."""
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def _list(value: YamlValue, context: str) -> list[YamlValue]:
    """Narrow one YAML sequence."""
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def _text(value: YamlValue, context: str) -> str:
    """Narrow one non-empty YAML string."""
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if not value:
        raise ValueError(f"{context} must be non-empty")
    return value


def _integer(value: YamlValue, context: str) -> int:
    """Narrow one non-negative YAML integer."""
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{context} must be an integer")
    if value < 0:
        raise ValueError(f"{context} must be non-negative")
    return value


def _boolean(value: YamlValue, context: str) -> bool:
    """Narrow one YAML boolean."""
    if not isinstance(value, bool):
        raise TypeError(f"{context} must be a boolean")
    return value


def _exact_keys(
    value: dict[str, YamlValue],
    required: set[str],
    optional: set[str],
    context: str,
) -> None:
    """Require all mandatory keys and reject unsupported keys."""
    missing = sorted(required - set(value))
    unsupported = sorted(set(value) - required - optional)
    if not missing and not unsupported:
        return
    details: list[str] = []
    if missing:
        details.append(f"missing {missing}")
    if unsupported:
        details.append(f"unsupported {unsupported}")
    raise ValueError(f"{context} has invalid keys: {', '.join(details)}")


def _string_tuple(value: YamlValue, context: str) -> tuple[str, ...]:
    """Decode a YAML string sequence."""
    return tuple(
        _text(item, f"{context}[{index}]")
        for index, item in enumerate(_list(value, context))
    )


def _nullable_string_tuple(value: YamlValue, context: str) -> tuple[str, ...]:
    """Decode a string sequence whose canonical empty form is YAML null."""
    if value is None:
        return ()
    return _string_tuple(value, context)


def _require_equal(actual: object, expected: object, context: str) -> None:
    """Reject a persisted projection that disagrees with canonical facts."""
    if actual != expected:
        raise ValueError(f"{context} is {actual}, expected {expected}")


def _decode_suite_reference(value: YamlValue, context: str) -> model.SuiteReference:
    """Decode one suite reference."""
    raw = _mapping(value, context)
    _exact_keys(raw, SUITE_REQUIRED_KEYS, SUITE_OPTIONAL_KEYS, context)
    precondition = (
        None
        if "precondition" not in raw
        else _text(raw["precondition"], f"{context}.precondition")
    )
    return model.SuiteReference(
        suite_id=_text(raw["id"], f"{context}.id"),
        file=_text(raw["file"], f"{context}.file"),
        precondition=precondition,
    )


def _decode_suite_references(
    value: YamlValue,
    context: str,
) -> tuple[model.SuiteReference, ...]:
    """Decode a canonically ordered suite-reference sequence."""
    suites = tuple(
        _decode_suite_reference(item, f"{context}[{index}]")
        for index, item in enumerate(_list(value, context))
    )
    if suites != tuple(sorted(set(suites))):
        raise ValueError(f"{context} must be unique and sorted")
    return suites


def _decode_suite_evidence(
    value: YamlValue,
    context: str,
) -> tuple[model.SuiteEvidence, ...]:
    """Decode canonically grouped suite evidence."""
    evidence: list[model.SuiteEvidence] = []
    for index, item in enumerate(_list(value, context)):
        item_context = f"{context}[{index}]"
        raw = _mapping(item, item_context)
        _exact_keys(
            raw,
            EVIDENCE_REQUIRED_KEYS,
            EVIDENCE_OPTIONAL_KEYS,
            item_context,
        )
        suite = _decode_suite_reference(
            {key: raw[key] for key in ("id", "file", "precondition") if key in raw},
            item_context,
        )
        evidence.append(
            model.SuiteEvidence(
                suite=suite,
                test_ids=_string_tuple(
                    raw["test_ids"],
                    f"{item_context}.test_ids",
                ),
            )
        )
    return tuple(evidence)


def _decode_evidence(
    asserted_value: YamlValue,
    mentioned_value: YamlValue,
    context: str,
) -> model.CoverageEvidence:
    """Decode one required or optional evidence partition."""
    return model.CoverageEvidence(
        asserted_in=_decode_suite_evidence(
            asserted_value,
            f"{context}.asserted_in",
        ),
        mentioned_only_in=_decode_suite_evidence(
            mentioned_value,
            f"{context}.mentioned_only_in",
        ),
    )


def _decode_conditionals(
    value: YamlValue,
    context: str,
) -> tuple[model.ConditionalCode, ...]:
    """Decode conditional-code projections, accepting null as empty."""
    if value is None:
        return ()
    result: list[model.ConditionalCode] = []
    for index, item in enumerate(_list(value, context)):
        item_context = f"{context}[{index}]"
        raw = _mapping(item, item_context)
        _exact_keys(raw, CONDITIONAL_KEYS, set(), item_context)
        result.append(
            model.ConditionalCode(
                code=_text(raw["code"], f"{item_context}.code"),
                precondition=_text(
                    raw["precondition"],
                    f"{item_context}.precondition",
                ),
            )
        )
    conditionals = tuple(result)
    if conditionals != tuple(sorted(set(conditionals), key=lambda item: item.code)):
        raise ValueError(f"{context} must be unique and sorted by code")
    return conditionals


def _decode_required_traces(
    value: YamlValue,
    context: str,
) -> tuple[_RequiredTrace, ...]:
    """Decode required-suite evidence and diagnostic obligations."""
    result: list[_RequiredTrace] = []
    for index, item in enumerate(_list(value, context)):
        item_context = f"{context}[{index}]"
        raw = _mapping(item, item_context)
        _exact_keys(
            raw,
            COVERAGE_REQUIRED_KEYS,
            COVERAGE_OPTIONAL_KEYS,
            item_context,
        )
        code = _text(raw["code"], f"{item_context}.code")
        severity = _text(raw["severity"], f"{item_context}.severity")
        if severity not in {"error", "warning", "note"}:
            raise ValueError(
                f"{item_context}.severity has unsupported value {severity!r}"
            )
        policy = _text(raw["policy"], f"{item_context}.policy")
        if policy not in diagnostic_model.COVERAGE_POLICIES:
            raise ValueError(f"{item_context}.policy has unsupported value {policy!r}")
        precondition = (
            None
            if "precondition" not in raw
            else _text(raw["precondition"], f"{item_context}.precondition")
        )
        evidence = _decode_evidence(
            raw["asserted_in"],
            raw["mentioned_only_in"],
            item_context,
        )
        status = _text(raw["status"], f"{item_context}.status")
        _require_equal(status, evidence.strength, f"{item_context}.status")
        result.append(
            _RequiredTrace(
                code=code,
                severity=severity,
                obligation=diagnostic_model.DiagnosticCoverageObligation(
                    policy=cast(diagnostic_model.CoveragePolicy, policy),
                    precondition=precondition,
                ),
                evidence=evidence,
            )
        )
    traces = tuple(result)
    codes = tuple(trace.code for trace in traces)
    if codes != tuple(sorted(set(codes))):
        raise ValueError(f"{context} codes must be unique and sorted")
    return traces


def _decode_optional_coverage(
    value: YamlValue,
    context: str,
) -> tuple[tuple[str, model.CoverageEvidence], ...]:
    """Decode optional evidence keyed by diagnostic code."""
    result: list[tuple[str, model.CoverageEvidence]] = []
    for index, item in enumerate(_list(value, context)):
        item_context = f"{context}[{index}]"
        raw = _mapping(item, item_context)
        _exact_keys(raw, OPTIONAL_COVERAGE_KEYS, set(), item_context)
        code = _text(raw["code"], f"{item_context}.code")
        evidence = _decode_evidence(
            raw["asserted_in"],
            raw["mentioned_only_in"],
            item_context,
        )
        expected_status = (
            "unasserted" if evidence.strength == model.UNCOVERED else evidence.strength
        )
        status = _text(raw["status"], f"{item_context}.status")
        _require_equal(status, expected_status, f"{item_context}.status")
        result.append((code, evidence))
    decoded = tuple(result)
    codes = tuple(code for code, _evidence in decoded)
    if codes != tuple(sorted(set(codes))):
        raise ValueError(f"{context} codes must be unique and sorted")
    return decoded


def _decode_contract_failures(
    value: YamlValue,
    context: str,
) -> tuple[model.ContractFailure, ...]:
    """Decode canonical contract validation failures."""
    failures: list[model.ContractFailure] = []
    for index, item in enumerate(_list(value, context)):
        item_context = f"{context}[{index}]"
        raw = _mapping(item, item_context)
        _exact_keys(raw, CONTRACT_FAILURE_KEYS, set(), item_context)
        failures.append(
            model.ContractFailure(
                code=_text(raw["code"], f"{item_context}.code"),
                suite_id=_text(raw["suite_id"], f"{item_context}.suite_id"),
                file=_text(raw["file"], f"{item_context}.file"),
                test_id=_text(raw["test_id"], f"{item_context}.test_id"),
                field=_text(raw["field"], f"{item_context}.field"),
                missing=_text(raw["missing"], f"{item_context}.missing"),
            )
        )
    result = tuple(failures)
    if result != tuple(sorted(result, key=lambda failure: failure.ordering_key)):
        raise ValueError(f"{context} must be sorted")
    return result


def _summary_values(report: model.TraceabilityReport) -> tuple[tuple[str, int], ...]:
    """Return artifact summary keys paired with derived values."""
    summary = report.summary
    return tuple(
        (key, cast(int, getattr(summary, attribute)))
        for key, attribute in SUMMARY_FIELDS
    )


def decode_report(
    document: YamlValue, context: str = "traceability"
) -> model.TraceabilityReport:
    """Decode one closed report and validate every persisted projection."""
    root = _mapping(document, context)
    _exact_keys(root, TRACEABILITY_KEYS, set(), context)

    version = _text(root["version"], f"{context}.version")
    _require_equal(version, model.VERSION, f"{context}.version")
    history_root = release_identity.validate_history_root(
        _text(root["release_history_root"], f"{context}.release_history_root"),
        f"{context}.release_history_root",
    )
    release = release_identity.parse_release_label(
        _text(root["release"], f"{context}.release"),
        f"{context}.release",
    )
    status = _text(root["status"], f"{context}.status")
    if status not in model.ARTIFACT_STATUSES:
        raise ValueError(
            f"{context}.status must be one of "
            f"{sorted(model.ARTIFACT_STATUSES)}, "
            f"found {status!r}"
        )
    coverage_scope = _text(root["coverage_scope"], f"{context}.coverage_scope")
    _require_equal(
        coverage_scope,
        model.COVERAGE_SCOPE,
        f"{context}.coverage_scope",
    )

    raw_sources = _mapping(root["sources"], f"{context}.sources")
    _exact_keys(raw_sources, SOURCE_KEYS, set(), f"{context}.sources")
    sources = model.TraceabilitySources(
        diagnostic_catalogs=_string_tuple(
            raw_sources["required_codes"],
            f"{context}.sources.required_codes",
        ),
        diagnostic_contract=_text(
            raw_sources["diagnostic_contract"],
            f"{context}.sources.diagnostic_contract",
        ),
        suites_manifest=_text(
            raw_sources["suites_manifest"],
            f"{context}.sources.suites_manifest",
        ),
    )

    raw_policy = _mapping(root["policy"], f"{context}.policy")
    _exact_keys(raw_policy, POLICY_KEYS, set(), f"{context}.policy")
    persisted_excluded_policy = _string_tuple(
        raw_policy["excluded_nonbaseline_codes"],
        f"{context}.policy.excluded_nonbaseline_codes",
    )
    persisted_conditionals = _decode_conditionals(
        raw_policy["conditional_required_codes"],
        f"{context}.policy.conditional_required_codes",
    )

    raw_summary = _mapping(root["summary"], f"{context}.summary")
    _exact_keys(raw_summary, _SUMMARY_KEYS, set(), f"{context}.summary")
    persisted_summary = {
        key: _integer(raw_summary[key], f"{context}.summary.{key}")
        for key, _attribute in SUMMARY_FIELDS
    }

    raw_contract_validation = _mapping(
        root["contract_validation"],
        f"{context}.contract_validation",
    )
    _exact_keys(
        raw_contract_validation,
        CONTRACT_VALIDATION_KEYS,
        set(),
        f"{context}.contract_validation",
    )
    failure_count = _integer(
        raw_contract_validation["failure_count"],
        f"{context}.contract_validation.failure_count",
    )
    contract_failures = _decode_contract_failures(
        raw_contract_validation["failures"],
        f"{context}.contract_validation.failures",
    )
    _require_equal(
        failure_count,
        len(contract_failures),
        f"{context}.contract_validation.failure_count",
    )

    required_traces = _decode_required_traces(
        root["coverage"],
        f"{context}.coverage",
    )

    raw_probe_summary = _mapping(
        root["optional_probe_summary"],
        f"{context}.optional_probe_summary",
    )
    _exact_keys(
        raw_probe_summary,
        OPTIONAL_PROBE_SUMMARY_KEYS,
        set(),
        f"{context}.optional_probe_summary",
    )
    probe_mode = _text(
        raw_probe_summary["mode"],
        f"{context}.optional_probe_summary.mode",
    )
    _require_equal(
        probe_mode,
        model.OPTIONAL_PROBE_MODE,
        f"{context}.optional_probe_summary.mode",
    )
    warning_threshold = _integer(
        raw_probe_summary["warning_threshold_min_asserted"],
        f"{context}.optional_probe_summary.warning_threshold_min_asserted",
    )
    persisted_warning_active = _boolean(
        raw_probe_summary["warning_active"],
        f"{context}.optional_probe_summary.warning_active",
    )
    persisted_warning_message = _text(
        raw_probe_summary["warning_message"],
        f"{context}.optional_probe_summary.warning_message",
    )
    persisted_probe_suites = _decode_suite_references(
        raw_probe_summary["optional_suites"],
        f"{context}.optional_probe_summary.optional_suites",
    )
    probe_coverage = _decode_optional_coverage(
        root["optional_probe_coverage"],
        f"{context}.optional_probe_coverage",
    )

    raw_extension_summary = _mapping(
        root["optional_extension_summary"],
        f"{context}.optional_extension_summary",
    )
    _exact_keys(
        raw_extension_summary,
        OPTIONAL_EXTENSION_SUMMARY_KEYS,
        set(),
        f"{context}.optional_extension_summary",
    )
    extension_mode = _text(
        raw_extension_summary["mode"],
        f"{context}.optional_extension_summary.mode",
    )
    _require_equal(
        extension_mode,
        model.OPTIONAL_EXTENSION_MODE,
        f"{context}.optional_extension_summary.mode",
    )
    persisted_extension_suites = _decode_suite_references(
        raw_extension_summary["optional_suites"],
        f"{context}.optional_extension_summary.optional_suites",
    )
    extension_coverage = _decode_optional_coverage(
        root["optional_extension_coverage"],
        f"{context}.optional_extension_coverage",
    )

    expected_probe_codes = tuple(
        trace.code
        for trace in required_traces
        if trace.obligation.policy == diagnostic_model.OPTIONAL_EMIT_IF_PRESENT
    )
    expected_extension_codes = tuple(
        trace.code
        for trace in required_traces
        if trace.obligation.policy == diagnostic_model.EXCLUDED_NONBASELINE
    )
    _require_equal(
        tuple(code for code, _evidence in probe_coverage),
        expected_probe_codes,
        f"{context}.optional_probe_coverage codes",
    )
    _require_equal(
        tuple(code for code, _evidence in extension_coverage),
        expected_extension_codes,
        f"{context}.optional_extension_coverage codes",
    )
    optional_evidence = dict(probe_coverage + extension_coverage)
    diagnostics = tuple(
        model.DiagnosticTrace(
            code=trace.code,
            severity=trace.severity,
            obligation=trace.obligation,
            required_suite_evidence=trace.evidence,
            optional_suite_evidence=optional_evidence.get(
                trace.code,
                model.EMPTY_COVERAGE,
            ),
        )
        for trace in required_traces
    )
    report = model.TraceabilityReport(
        release_history_root=history_root,
        release=release,
        status=cast(model.ArtifactStatus, status),
        sources=sources,
        diagnostics=diagnostics,
        contract_failures=contract_failures,
        optional_probe_policy=model.OptionalProbePolicy(
            warning_threshold_min_asserted=warning_threshold
        ),
    )

    for key, expected in _summary_values(report):
        _require_equal(
            persisted_summary[key],
            expected,
            f"{context}.summary.{key}",
        )
    _require_equal(
        persisted_excluded_policy,
        report.excluded_nonbaseline_codes,
        f"{context}.policy.excluded_nonbaseline_codes",
    )
    _require_equal(
        persisted_conditionals,
        report.conditional_required_codes,
        f"{context}.policy.conditional_required_codes",
    )
    _require_equal(
        persisted_probe_suites,
        report.optional_probe_suites,
        f"{context}.optional_probe_summary.optional_suites",
    )
    _require_equal(
        persisted_warning_active,
        report.optional_probe_warning_active,
        f"{context}.optional_probe_summary.warning_active",
    )
    _require_equal(
        persisted_warning_message,
        report.optional_probe_warning_message,
        f"{context}.optional_probe_summary.warning_message",
    )
    _require_equal(
        persisted_extension_suites,
        report.optional_extension_suites,
        f"{context}.optional_extension_summary.optional_suites",
    )
    _require_equal(
        _string_tuple(root["uncovered_codes"], f"{context}.uncovered_codes"),
        report.uncovered_codes,
        f"{context}.uncovered_codes",
    )
    _require_equal(
        _nullable_string_tuple(root["hard_gaps"], f"{context}.hard_gaps"),
        report.hard_gaps,
        f"{context}.hard_gaps",
    )
    _require_equal(
        _decode_conditionals(
            root["conditional_pending"],
            f"{context}.conditional_pending",
        ),
        report.conditional_pending,
        f"{context}.conditional_pending",
    )
    _require_equal(
        _string_tuple(
            root["optional_emit_unasserted"],
            f"{context}.optional_emit_unasserted",
        ),
        report.optional_emit_unasserted,
        f"{context}.optional_emit_unasserted",
    )
    _require_equal(
        _string_tuple(
            root["excluded_nonbaseline_codes"],
            f"{context}.excluded_nonbaseline_codes",
        ),
        report.excluded_nonbaseline_codes,
        f"{context}.excluded_nonbaseline_codes",
    )
    return report


def _emit_suite_evidence(
    lines: list[str],
    evidence: tuple[model.SuiteEvidence, ...],
) -> None:
    """Append suite evidence using the historical nested layout."""
    for item in evidence:
        lines.append(f"      - id: {item.suite.suite_id}")
        lines.append(f"        file: {item.suite.file}")
        if item.suite.precondition is not None:
            lines.append(f"        precondition: {item.suite.precondition}")
        lines.append(f"        test_ids: [{', '.join(item.test_ids)}]")


def _emit_optional_suites(
    lines: list[str],
    suites: tuple[model.SuiteReference, ...],
) -> None:
    """Append optional suite references."""
    if not suites:
        lines.append("    []")
        return
    for suite in suites:
        lines.append(f"    - id: {suite.suite_id}")
        lines.append(f"      file: {suite.file}")
        if suite.precondition is not None:
            lines.append(f"      precondition: {suite.precondition}")


def _emit_contract_failures(
    lines: list[str],
    failures: tuple[model.ContractFailure, ...],
) -> None:
    """Append contract failures with safe quoted details."""
    if not failures:
        lines.append("    []")
        return
    for failure in failures:
        lines.append(f"    - code: {failure.code}")
        lines.append(f"      suite_id: {failure.suite_id}")
        lines.append(f"      file: {failure.file}")
        lines.append(f"      test_id: {failure.test_id}")
        lines.append(f"      field: {failure.field}")
        lines.append(f"      missing: {json.dumps(failure.missing)}")


def _emit_coverage_evidence(
    lines: list[str],
    evidence: model.CoverageEvidence,
    *,
    optional: bool,
) -> None:
    """Append a status and the corresponding evidence branch."""
    status = (
        "unasserted"
        if optional and evidence.strength == model.UNCOVERED
        else evidence.strength
    )
    lines.append(f"    status: {status}")
    if evidence.asserted_in:
        lines.append("    asserted_in:")
        _emit_suite_evidence(lines, evidence.asserted_in)
        lines.append("    mentioned_only_in: []")
    elif evidence.mentioned_only_in:
        lines.append("    asserted_in: []")
        lines.append("    mentioned_only_in:")
        _emit_suite_evidence(lines, evidence.mentioned_only_in)
    else:
        lines.append("    asserted_in: []")
        lines.append("    mentioned_only_in: []")


def _emit_string_list(lines: list[str], key: str, values: tuple[str, ...]) -> None:
    """Append one block string list using the historical empty representation."""
    lines.append(f"{key}:")
    for value in values:
        lines.append(f"  - {value}")


def encode_report(report: model.TraceabilityReport) -> bytes:
    """Encode one report to the exact checked-in UTF-8 YAML layout."""
    lines = [
        "# Generated machine-checkable mapping: required diagnostic code -> conformance tests",
        f"version: '{model.VERSION}'",
        f"release_history_root: {report.release_history_root}",
        f"release: {report.release.text}",
        f"status: {report.status}",
        f"coverage_scope: {model.COVERAGE_SCOPE}",
        "sources:",
        "  required_codes:",
    ]
    lines.extend(f"    - {path}" for path in report.sources.diagnostic_catalogs)
    lines.extend(
        (
            f"  diagnostic_contract: {report.sources.diagnostic_contract}",
            f"  suites_manifest: {report.sources.suites_manifest}",
            "policy:",
            "  excluded_nonbaseline_codes:",
        )
    )
    lines.extend(f"    - {code}" for code in report.excluded_nonbaseline_codes)
    lines.append("  conditional_required_codes:")
    for item in report.conditional_required_codes:
        lines.append(f"    - code: {item.code}")
        lines.append(f"      precondition: {item.precondition}")

    lines.append("summary:")
    for key, value in _summary_values(report):
        lines.append(f"  {key}: {value}")

    lines.extend(
        (
            "contract_validation:",
            f"  failure_count: {len(report.contract_failures)}",
            "  failures:",
        )
    )
    _emit_contract_failures(lines, report.contract_failures)

    lines.append("coverage:")
    for trace in report.diagnostics:
        lines.append(f"  - code: {trace.code}")
        lines.append(f"    severity: {trace.severity}")
        lines.append(f"    policy: {trace.obligation.policy}")
        if trace.obligation.precondition is not None:
            lines.append(f"    precondition: {trace.obligation.precondition}")
        _emit_coverage_evidence(
            lines,
            trace.required_suite_evidence,
            optional=False,
        )

    lines.extend(
        (
            "optional_probe_summary:",
            f"  mode: {model.OPTIONAL_PROBE_MODE}",
            (
                "  warning_threshold_min_asserted: "
                f"{report.optional_probe_policy.warning_threshold_min_asserted}"
            ),
            (
                "  warning_active: "
                f"{'true' if report.optional_probe_warning_active else 'false'}"
            ),
            f"  warning_message: {json.dumps(report.optional_probe_warning_message)}",
            "  optional_suites:",
        )
    )
    _emit_optional_suites(lines, report.optional_probe_suites)
    lines.append("optional_probe_coverage:")
    for trace in report.diagnostics:
        if trace.obligation.policy != diagnostic_model.OPTIONAL_EMIT_IF_PRESENT:
            continue
        lines.append(f"  - code: {trace.code}")
        _emit_coverage_evidence(
            lines,
            trace.optional_suite_evidence,
            optional=True,
        )

    lines.extend(
        (
            "optional_extension_summary:",
            f"  mode: {model.OPTIONAL_EXTENSION_MODE}",
            "  optional_suites:",
        )
    )
    _emit_optional_suites(lines, report.optional_extension_suites)
    lines.append("optional_extension_coverage:")
    for trace in report.diagnostics:
        if trace.obligation.policy != diagnostic_model.EXCLUDED_NONBASELINE:
            continue
        lines.append(f"  - code: {trace.code}")
        _emit_coverage_evidence(
            lines,
            trace.optional_suite_evidence,
            optional=True,
        )

    _emit_string_list(lines, "uncovered_codes", report.uncovered_codes)
    _emit_string_list(lines, "hard_gaps", report.hard_gaps)
    lines.append("conditional_pending:")
    for item in report.conditional_pending:
        lines.append(f"  - code: {item.code}")
        lines.append(f"    precondition: {item.precondition}")
    _emit_string_list(
        lines,
        "optional_emit_unasserted",
        report.optional_emit_unasserted,
    )
    _emit_string_list(
        lines,
        "excluded_nonbaseline_codes",
        report.excluded_nonbaseline_codes,
    )
    return ("\n".join(lines) + "\n").encode("utf-8")

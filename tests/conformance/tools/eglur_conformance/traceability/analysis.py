"""Derive canonical diagnostic trace facts from normalized source models."""

import re
from collections import defaultdict
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from typing import cast

from eglur_conformance import release_identity
from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.suite import model as suite_model
from eglur_conformance.traceability import model

_RE_CODE_HEADER = re.compile(r"^###\s+`([A-Z][A-Z0-9_]+)`\s*$")
_RE_UPPER_TOKEN = re.compile(r"\b[A-Z][A-Z0-9_]{2,}\b")
_RE_SEVERITY = re.compile(r"^Severity:\s*`(error|warning|note)`\s*$")


@dataclass(frozen=True)
class _TestEvidence:
    """One conformance-test evidence location."""

    identity: suite_model.TestIdentity
    file: str
    suite_precondition: str | None


@dataclass(frozen=True)
class CodeMetadata:
    """Metadata for one diagnostic code."""

    source_file: str
    severity: model.DiagnosticSeverity


def extract_code_metadata(
    source_file: str,
    visible_lines: Iterable[tuple[int, str]],
) -> dict[str, CodeMetadata]:
    """Extract diagnostic metadata from visible Markdown lines."""
    metadata: dict[str, CodeMetadata] = {}
    current_code: str | None = None

    for _line_number, raw in visible_lines:
        line = raw.strip()
        matched_code = _RE_CODE_HEADER.match(line)
        if matched_code:
            current_code = matched_code.group(1)
            continue

        matched_severity = _RE_SEVERITY.match(line)
        if matched_severity and current_code is not None:
            metadata[current_code] = CodeMetadata(
                source_file=source_file,
                severity=cast(
                    model.DiagnosticSeverity,
                    matched_severity.group(1),
                ),
            )
            current_code = None

    return metadata


def _collect_test_evidence(
    suite: suite_model.ConformanceSuite,
    required_codes: set[str],
    contracts: dict[str, diagnostic_contracts.DiagnosticContract],
    channel_registry: diagnostic_model.DiagnosticRegistry | None = None,
) -> tuple[
    dict[str, list[_TestEvidence]],
    dict[str, list[_TestEvidence]],
    list[model.ContractFailure],
]:
    """Collect evidence and diagnostic contract failures from one suite."""
    entry = suite.entry
    rel_file = entry.file
    asserted_by_code: dict[str, list[_TestEvidence]] = defaultdict(list)
    mentioned_by_code: dict[str, list[_TestEvidence]] = defaultdict(list)
    contract_failures: list[model.ContractFailure] = []

    def add_failure(
        code: str,
        test_id: str,
        field: str,
        detail: str,
    ) -> None:
        contract_failures.append(
            model.ContractFailure(
                code=code,
                suite_id=entry.suite_id,
                file=rel_file,
                test_id=test_id,
                field=field,
                missing=detail,
            )
        )

    def template_matches(
        token: str,
        template: diagnostic_contracts.MessageTemplate,
    ) -> bool:
        pattern = re.escape(template.pattern)
        for field in template.fields:
            pattern = pattern.replace(re.escape(f"{{{field}}}"), r".+")
        return re.fullmatch(pattern, token) is not None

    def validate_diagnostic(
        diagnostic: suite_model.DiagnosticAssertion,
        test_id: str,
        channel: str,
    ) -> str | None:
        code = diagnostic.code
        contract = contracts.get(code)
        if contract is None:
            add_failure(code, test_id, "code", "<unknown diagnostic code>")
            return code
        if channel_registry is not None:
            registry_entry = channel_registry.get(code)
            if registry_entry is None:
                add_failure(code, test_id, "channel", "<missing channel contract>")
            elif channel not in registry_entry.channels.allowed_channels:
                add_failure(
                    code,
                    test_id,
                    "channel",
                    f"{channel} not in {sorted(registry_entry.channels.allowed_channels)}",
                )
        if code not in required_codes:
            add_failure(code, test_id, "catalog", "<missing diagnostic catalog entry>")
            return code

        message_tokens = diagnostic.message_all_of
        missing_message_tokens = set(contract.message_all_of) - set(message_tokens)
        for token in sorted(missing_message_tokens):
            add_failure(code, test_id, "message_contains", token)

        template_tokens = {
            token
            for token in message_tokens
            if any(
                template_matches(token, template)
                for template in contract.message_templates
            )
        }
        for template in contract.message_templates:
            if not any(template_matches(token, template) for token in message_tokens):
                add_failure(
                    code,
                    test_id,
                    "message_contains",
                    f"template {template.pattern}",
                )

        if entry.required:
            allowed_tokens = set(contract.message_all_of) | template_tokens
            for token in sorted(set(message_tokens) - allowed_tokens):
                add_failure(
                    code,
                    test_id,
                    "message_contains",
                    f"unexpected non-portable token {token!r}",
                )

        actual_any_of = {
            frozenset(group) for group in diagnostic.message_any_of if group
        }
        expected_any_of = {frozenset(group) for group in contract.message_any_of}
        if entry.required and actual_any_of != expected_any_of:
            add_failure(
                code,
                test_id,
                "message_contains_any",
                f"expected {sorted(map(sorted, expected_any_of))}, "
                f"found {sorted(map(sorted, actual_any_of))}",
            )
        elif not entry.required:
            for missing_group in expected_any_of - actual_any_of:
                add_failure(
                    code,
                    test_id,
                    "message_contains_any",
                    f"missing one-of group {sorted(missing_group)}",
                )

        if entry.required:
            for field, value in (
                ("help_contains", diagnostic.help_contains),
                ("related_contains", diagnostic.related_contains),
            ):
                if value is not None:
                    add_failure(
                        code,
                        test_id,
                        field,
                        "field is optional and forbidden in required suites",
                    )

        data_keys = diagnostic.data_keys
        data_equals = dict(diagnostic.data_equals)
        required_data_keys = contract.required_data_keys(data_equals)
        if entry.required:
            selector_keys = {
                condition.key for condition in contract.data_conditional_required
            }
            for key in sorted(
                selector_keys.intersection(required_data_keys) - data_equals.keys()
            ):
                add_failure(
                    code,
                    test_id,
                    "data_equals",
                    f"{key} selects required conditional data",
                )
            if set(data_keys) != set(required_data_keys):
                add_failure(
                    code,
                    test_id,
                    "data_keys",
                    f"expected {sorted(required_data_keys)}, found {sorted(data_keys)}",
                )
        else:
            for key in sorted(set(required_data_keys) - set(data_keys)):
                add_failure(code, test_id, "data_keys", key)
            if data_keys:
                optional_missing = set(contract.data_required_if_emitted) - set(
                    data_keys
                )
                for key in sorted(optional_missing):
                    add_failure(code, test_id, "data_keys", key)

        for key in data_equals:
            if key not in data_keys:
                add_failure(
                    code, test_id, "data_equals", f"{key} not listed in data_keys"
                )
            if entry.required and key not in contract.data_exact_value_keys:
                add_failure(
                    code,
                    test_id,
                    "data_equals",
                    f"{key} has no normative exact-value contract",
                )
        return code

    for test in suite.document.tests:
        test_id = test.test_id
        evidence = _TestEvidence(
            identity=suite_model.TestIdentity(entry.suite_id, test_id),
            file=rel_file,
            suite_precondition=entry.precondition,
        )
        asserted: set[str] = set()
        mentioned: set[str] = set()

        mentioned.update(
            token
            for token in _RE_UPPER_TOKEN.findall(test.title)
            if token in required_codes
        )

        expectation = test.expectation
        diagnostics: list[tuple[str, suite_model.DiagnosticAssertion]] = []
        if isinstance(expectation.outcome, suite_model.CompileErrorOutcome):
            diagnostics.append(
                (diagnostic_model.COMPILE_ERROR, expectation.outcome.diagnostic)
            )
        elif isinstance(expectation.outcome, suite_model.RuntimeErrorOutcome):
            diagnostics.append(
                (diagnostic_model.RUNTIME_ERROR, expectation.outcome.diagnostic)
            )
        diagnostics.extend(
            (diagnostic_model.VALUE_DIAGNOSTICS, diagnostic)
            for diagnostic in expectation.value_diagnostics
        )

        if expectation.diagnostic_contains is not None:
            for item in expectation.diagnostic_contains:
                mentioned.update(
                    token
                    for token in _RE_UPPER_TOKEN.findall(item)
                    if token in required_codes
                )
            if entry.required:
                for code in [item.code for _, item in diagnostics] or ["<unscoped>"]:
                    add_failure(
                        code,
                        test_id,
                        "diagnostic_contains",
                        "legacy unscoped field is forbidden in required suites",
                    )

        if isinstance(expectation.outcome, suite_model.TestRunOutcome):
            for failure in expectation.outcome.failed:
                if failure.kind != "runtime_abort" or failure.runtime_code is None:
                    continue
                runtime_code = failure.runtime_code
                if runtime_code not in contracts:
                    add_failure(
                        runtime_code,
                        test_id,
                        "code",
                        "<unknown diagnostic code>",
                    )
                else:
                    registry_entry = (
                        channel_registry.get(runtime_code)
                        if channel_registry is not None
                        else None
                    )
                    if channel_registry is not None and registry_entry is None:
                        add_failure(
                            runtime_code,
                            test_id,
                            "channel",
                            "<missing channel contract>",
                        )
                    elif (
                        registry_entry is not None
                        and diagnostic_model.RUNTIME_ERROR
                        not in registry_entry.channels.allowed_channels
                    ):
                        add_failure(
                            runtime_code,
                            test_id,
                            "channel",
                            "runtime_error not in "
                            f"{sorted(registry_entry.channels.allowed_channels)}",
                        )
                if runtime_code not in required_codes:
                    add_failure(
                        runtime_code,
                        test_id,
                        "catalog",
                        "<missing diagnostic catalog entry>",
                    )
                # Test-run records lack the full diagnostic data contract,
                # so their runtime code is validated but is not coverage.

        for channel, diagnostic in diagnostics:
            validated_code = validate_diagnostic(diagnostic, test_id, channel)
            if validated_code is not None:
                asserted.add(validated_code)
                mentioned.add(validated_code)

        for code in sorted(asserted):
            asserted_by_code[code].append(evidence)
        for code in sorted(mentioned - asserted):
            mentioned_by_code[code].append(evidence)

    return asserted_by_code, mentioned_by_code, contract_failures


def validate_contract_catalog_alignment(
    contracts: Mapping[str, diagnostic_contracts.DiagnosticContract],
    metadata: Mapping[str, CodeMetadata],
    contract_file: str,
) -> list[model.ContractFailure]:
    """Require machine contracts and Markdown code catalogs to stay aligned."""
    failures: list[model.ContractFailure] = []
    for code in sorted(set(metadata) - set(contracts)):
        failures.append(
            model.ContractFailure(
                code=code,
                suite_id="<contract>",
                file=contract_file,
                test_id="<catalog-alignment>",
                field="contract",
                missing="code is missing from machine contract",
            )
        )
    for code in sorted(set(contracts) - set(metadata)):
        failures.append(
            model.ContractFailure(
                code=code,
                suite_id="<contract>",
                file=contract_file,
                test_id="<catalog-alignment>",
                field="contract",
                missing="code is missing from Markdown catalogs",
            )
        )
    for code in sorted(set(contracts) & set(metadata)):
        if contracts[code].severity != metadata[code].severity:
            failures.append(
                model.ContractFailure(
                    code=code,
                    suite_id="<contract>",
                    file=contract_file,
                    test_id="<catalog-alignment>",
                    field="severity",
                    missing=(
                        f"contract={contracts[code].severity}, "
                        f"catalog={metadata[code].severity}"
                    ),
                )
            )
    return failures


def _group_evidence(
    evidences: Iterable[_TestEvidence],
) -> tuple[model.SuiteEvidence, ...]:
    """Group test-level evidence under canonical suite references."""
    grouped: dict[model.SuiteReference, list[str]] = defaultdict(list)
    for evidence in evidences:
        suite = model.SuiteReference(
            suite_id=evidence.identity.suite_id,
            file=evidence.file,
            precondition=evidence.suite_precondition,
        )
        grouped[suite].append(evidence.identity.test_id)
    return tuple(
        model.SuiteEvidence(
            suite=suite,
            test_ids=tuple(sorted(set(test_ids))),
        )
        for suite, test_ids in sorted(grouped.items())
    )


def _coverage_evidence(
    code: str,
    asserted: Mapping[str, list[_TestEvidence]],
    mentioned_only: Mapping[str, list[_TestEvidence]],
) -> model.CoverageEvidence:
    """Collapse evidence to its strongest canonical representation."""
    asserted_entries = _group_evidence(asserted.get(code, ()))
    if asserted_entries:
        return model.CoverageEvidence(
            asserted_in=asserted_entries,
            mentioned_only_in=(),
        )
    return model.CoverageEvidence(
        asserted_in=(),
        mentioned_only_in=_group_evidence(mentioned_only.get(code, ())),
    )


def build_traceability_report(
    bundle: suite_model.ConformanceBundle,
    code_metadata: Mapping[str, CodeMetadata],
    contracts: dict[str, diagnostic_contracts.DiagnosticContract],
    channel_registry: diagnostic_model.DiagnosticRegistry,
    sources: model.TraceabilitySources,
    optional_probe_policy: model.OptionalProbePolicy,
    contract_file: str,
) -> model.TraceabilityReport:
    """Aggregate normalized catalogs, contracts, and suites into one report."""
    required_codes = tuple(sorted(code_metadata))
    required_code_set = set(required_codes)
    missing_profiles = sorted(required_code_set - set(channel_registry))
    if missing_profiles:
        raise ValueError(
            f"diagnostic registry is missing catalog codes: {missing_profiles}"
        )

    required_asserted: dict[str, list[_TestEvidence]] = defaultdict(list)
    required_mentioned_only: dict[str, list[_TestEvidence]] = defaultdict(list)
    optional_asserted: dict[str, list[_TestEvidence]] = defaultdict(list)
    optional_mentioned_only: dict[str, list[_TestEvidence]] = defaultdict(list)
    contract_failures = validate_contract_catalog_alignment(
        contracts,
        code_metadata,
        contract_file,
    )

    for suite in sorted(bundle.suites, key=lambda item: item.entry.file):
        asserted, mentioned, suite_failures = _collect_test_evidence(
            suite,
            required_code_set,
            contracts,
            channel_registry,
        )
        contract_failures.extend(suite_failures)
        target_asserted = (
            required_asserted if suite.entry.required else optional_asserted
        )
        target_mentioned = (
            required_mentioned_only if suite.entry.required else optional_mentioned_only
        )
        for code, evidences in asserted.items():
            target_asserted[code].extend(evidences)
        for code, evidences in mentioned.items():
            target_mentioned[code].extend(evidences)

    diagnostics: list[model.DiagnosticTrace] = []
    for code in required_codes:
        obligation = channel_registry[code].coverage
        optional_evidence = (
            _coverage_evidence(
                code,
                optional_asserted,
                optional_mentioned_only,
            )
            if obligation.policy
            in {
                diagnostic_model.OPTIONAL_EMIT_IF_PRESENT,
                diagnostic_model.EXCLUDED_NONBASELINE,
            }
            else model.EMPTY_COVERAGE
        )
        diagnostics.append(
            model.DiagnosticTrace(
                code=code,
                severity=code_metadata[code].severity,
                obligation=obligation,
                required_suite_evidence=_coverage_evidence(
                    code,
                    required_asserted,
                    required_mentioned_only,
                ),
                optional_suite_evidence=optional_evidence,
            )
        )

    status = bundle.manifest.status
    if status not in model.ARTIFACT_STATUSES:
        raise ValueError(f"unsupported traceability status {status!r}")
    return model.TraceabilityReport(
        release_history_root=release_identity.validate_history_root(
            bundle.manifest.release_history_root,
            "traceability.release_history_root",
        ),
        release=release_identity.parse_release_label(
            bundle.manifest.release,
            "traceability.release",
        ),
        status=cast(model.ArtifactStatus, status),
        sources=sources,
        diagnostics=tuple(diagnostics),
        contract_failures=tuple(
            sorted(contract_failures, key=lambda failure: failure.ordering_key)
        ),
        optional_probe_policy=optional_probe_policy,
    )

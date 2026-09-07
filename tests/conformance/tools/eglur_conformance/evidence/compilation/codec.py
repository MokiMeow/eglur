"""Decode and address compilation evidence artifacts."""

import json
import re
from typing import cast

from eglur_conformance import release_identity
from eglur_conformance.artifact_content import (
    SCHEMA_VERSION,
    JsonValue,
    canonical_digest,
    expect_concrete_string,
    expect_digest,
    expect_exact_keys,
    expect_list,
    expect_mapping,
    expect_non_negative_int,
    expect_positive_int,
    expect_text,
    snapshot_bytes,
)
from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.evidence.compilation import model as compile_model
from eglur_conformance.yaml_document import YamlValue

_COMPILE_OBSERVATION_KIND = "eglur_compile_observation"
_COMPILE_EVIDENCE_BINDING_KIND = "eglur_compile_evidence_binding"
_COMPILER_RESULT_KIND = "eglur_compile_result"
_COMPILE_OUTCOMES = frozenset({"rejected", "succeeded"})
_PROCESS_TERMINATION_KINDS = frozenset({"exited", "signaled"})
_SCALAR_KINDS = diagnostic_contracts.SCALAR_KINDS
MAX_COMPILER_RESULT_BYTES = 1_048_576
MAX_COMPILER_RESULT_JSON_DEPTH = 32
_GIT_COMMIT_PATTERN = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
_DIAGNOSTIC_CODE_PATTERN = re.compile(r"[A-Z][A-Z0-9_]+\Z")


def expect_git_commit(value: YamlValue, context: str) -> str:
    """Return a full lowercase Git object ID.

    Parameters
    ----------
    value : YamlValue
        Candidate serialized commit.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    str
        Validated 40- or 64-digit lowercase object ID.

    Raises
    ------
    TypeError
        ``value`` is not a string.
    ValueError
        ``value`` is not a full lowercase Git object ID.
    """
    commit = expect_concrete_string(value, context)
    if _GIT_COMMIT_PATTERN.fullmatch(commit) is None:
        raise ValueError(f"{context} must be a full lowercase Git commit ID")
    return commit


def parse_diagnostic_scalar(
    value: YamlValue,
    context: str,
) -> compile_model.DiagnosticScalar:
    """Parse a closed tagged diagnostic scalar.

    Parameters
    ----------
    value : YamlValue
        Candidate tagged scalar mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    compile_model.DiagnosticScalar
        Validated scalar and explicit kind.

    Raises
    ------
    TypeError
        The representation or scalar has the wrong type.
    ValueError
        The representation is open, malformed, or kind-inconsistent.
    """
    scalar = expect_mapping(value, context)
    kind = expect_concrete_string(scalar.get("kind"), f"{context}.kind")
    if kind not in _SCALAR_KINDS:
        raise ValueError(f"{context}.kind must be one of {sorted(_SCALAR_KINDS)}")
    expect_exact_keys(scalar, {"kind", "value"}, context)
    parsed_value = diagnostic_contracts.expect_diagnostic_scalar(
        scalar.get("value"),
        f"{context}.value",
    )
    actual_kind = diagnostic_contracts.diagnostic_scalar_kind(parsed_value)
    if actual_kind != kind:
        raise ValueError(f"{context}.value has kind {actual_kind!r}, expected {kind!r}")
    return compile_model.DiagnosticScalar(kind=kind, value=parsed_value)


def _diagnostic_scalar_payload(
    value: compile_model.DiagnosticScalar,
) -> dict[str, JsonValue]:
    """Return one tagged diagnostic scalar as canonical JSON data."""
    return {"kind": value.kind, "value": value.value}


def _parse_source_coordinate(
    value: YamlValue,
    context: str,
) -> diagnostic_model.SourceCoordinate:
    coordinate = expect_mapping(value, context)
    expect_exact_keys(coordinate, {"line", "column"}, context)
    try:
        return diagnostic_model.SourceCoordinate(
            line=expect_positive_int(coordinate.get("line"), f"{context}.line"),
            column=expect_positive_int(
                coordinate.get("column"),
                f"{context}.column",
            ),
        )
    except ValueError as error:
        raise ValueError(f"{context}.{error}") from error


def _source_coordinate_payload(
    coordinate: diagnostic_model.SourceCoordinate,
) -> dict[str, JsonValue]:
    return {"line": coordinate.line, "column": coordinate.column}


def _parse_text_range(value: YamlValue, context: str) -> diagnostic_model.TextRange:
    text_range = expect_mapping(value, context)
    expect_exact_keys(text_range, {"start", "end"}, context)
    try:
        return diagnostic_model.TextRange(
            start=_parse_source_coordinate(
                text_range.get("start"),
                f"{context}.start",
            ),
            end=_parse_source_coordinate(text_range.get("end"), f"{context}.end"),
        )
    except ValueError as error:
        raise ValueError(f"{context}.{error}") from error


def _text_range_payload(text_range: diagnostic_model.TextRange) -> dict[str, JsonValue]:
    return {
        "start": _source_coordinate_payload(text_range.start),
        "end": _source_coordinate_payload(text_range.end),
    }


def _parse_source_span(value: YamlValue, context: str) -> diagnostic_model.SourceSpan:
    span = expect_mapping(value, context)
    required = {"kind", "source", "bytes"}
    allowed = required | {"text"}
    if set(span) - allowed or required - set(span):
        expect_exact_keys(span, required if "text" not in span else allowed, context)
    kind = expect_concrete_string(span.get("kind"), f"{context}.kind")
    if kind != "source":
        raise ValueError(f"{context}.kind must be 'source'")
    byte_range = expect_mapping(span.get("bytes"), f"{context}.bytes")
    expect_exact_keys(byte_range, {"start", "end"}, f"{context}.bytes")
    try:
        return diagnostic_model.SourceSpan(
            source=expect_text(
                span.get("source"),
                f"{context}.source",
                allow_empty=False,
            ),
            byte_range=diagnostic_model.ByteRange(
                start=expect_non_negative_int(
                    byte_range.get("start"),
                    f"{context}.bytes.start",
                ),
                end=expect_non_negative_int(
                    byte_range.get("end"),
                    f"{context}.bytes.end",
                ),
            ),
            text_range=(
                None
                if "text" not in span
                else _parse_text_range(span.get("text"), f"{context}.text")
            ),
        )
    except ValueError as error:
        raise ValueError(f"{context}.{error}") from error


def _source_span_payload(span: diagnostic_model.SourceSpan) -> dict[str, JsonValue]:
    payload: dict[str, JsonValue] = {
        "kind": "source",
        "source": span.source,
        "bytes": {
            "start": span.byte_range.start,
            "end": span.byte_range.end,
        },
    }
    if span.text_range is not None:
        payload["text"] = _text_range_payload(span.text_range)
    return payload


def _parse_primary_span(
    value: YamlValue,
    context: str,
) -> diagnostic_model.PrimarySpan:
    span = expect_mapping(value, context)
    kind = expect_concrete_string(span.get("kind"), f"{context}.kind")
    if kind == "unavailable":
        expect_exact_keys(span, {"kind"}, context)
        return diagnostic_model.UnavailableSpan()
    if kind != "source":
        raise ValueError(f"{context}.kind must be 'source' or 'unavailable'")
    return _parse_source_span(value, context)


def _primary_span_payload(
    span: diagnostic_model.PrimarySpan,
) -> dict[str, JsonValue]:
    if isinstance(span, diagnostic_model.UnavailableSpan):
        return {"kind": "unavailable"}
    return _source_span_payload(span)


def _parse_observed_diagnostic(
    value: YamlValue,
    context: str,
) -> compile_model.ObservedDiagnostic:
    """Parse one primary compiler diagnostic under a closed schema."""
    diagnostic = expect_mapping(value, context)
    expect_exact_keys(
        diagnostic,
        {
            "severity",
            "code",
            "message",
            "data",
            "primary_span",
        },
        context,
    )
    severity = expect_concrete_string(
        diagnostic.get("severity"),
        f"{context}.severity",
    )
    if severity != "error":
        raise ValueError(f"{context}.severity must be 'error'")
    code = expect_concrete_string(diagnostic.get("code"), f"{context}.code")
    if _DIAGNOSTIC_CODE_PATTERN.fullmatch(code) is None:
        raise ValueError(f"{context}.code must be uppercase snake case")
    raw_data = expect_mapping(diagnostic.get("data"), f"{context}.data")
    data = tuple(
        (
            expect_text(key, f"{context}.data key", allow_empty=False),
            parse_diagnostic_scalar(raw_value, f"{context}.data.{key}"),
        )
        for key, raw_value in sorted(raw_data.items())
    )
    return compile_model.ObservedDiagnostic(
        severity=severity,
        code=code,
        message=expect_text(
            diagnostic.get("message"),
            f"{context}.message",
            allow_empty=False,
        ),
        data=data,
        primary_span=_parse_primary_span(
            diagnostic.get("primary_span"),
            f"{context}.primary_span",
        ),
    )


def _observed_diagnostic_payload(
    diagnostic: compile_model.ObservedDiagnostic,
) -> dict[str, JsonValue]:
    """Return one primary diagnostic as canonical JSON data."""
    return {
        "severity": diagnostic.severity,
        "code": diagnostic.code,
        "message": diagnostic.message,
        "data": {
            key: _diagnostic_scalar_payload(value) for key, value in diagnostic.data
        },
        "primary_span": _primary_span_payload(diagnostic.primary_span),
    }


def _parse_compile_outcome(
    value: YamlValue, context: str
) -> compile_model.CompileOutcome:
    """Parse one succeeded/rejected compile-phase outcome."""
    outcome = expect_mapping(value, context)
    kind = expect_concrete_string(outcome.get("kind"), f"{context}.kind")
    if kind not in _COMPILE_OUTCOMES:
        raise ValueError(f"{context}.kind must be one of {sorted(_COMPILE_OUTCOMES)}")
    if kind == "succeeded":
        expect_exact_keys(outcome, {"kind"}, context)
        return compile_model.CompileSucceeded()
    expect_exact_keys(outcome, {"kind", "primary_diagnostic"}, context)
    return compile_model.CompileRejected(
        primary_diagnostic=_parse_observed_diagnostic(
            outcome.get("primary_diagnostic"),
            f"{context}.primary_diagnostic",
        ),
    )


def _compile_outcome_payload(
    outcome: compile_model.CompileOutcome,
) -> dict[str, JsonValue]:
    """Return one compile outcome as canonical JSON data."""
    if isinstance(outcome, compile_model.CompileSucceeded):
        return {"kind": "succeeded"}
    return {
        "kind": "rejected",
        "primary_diagnostic": _observed_diagnostic_payload(outcome.primary_diagnostic),
    }


def parse_compiler_result(
    value: YamlValue,
    context: str = "compiler_result",
) -> compile_model.CompilerResult:
    """Parse a closed compiler-native result document.

    Parameters
    ----------
    value : YamlValue
        Candidate compiler-result mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    compile_model.CompilerResult
        Validated compiler result.

    Raises
    ------
    TypeError
        A result field has the wrong type.
    ValueError
        The result violates the closed compiler-result schema.
    """
    result = expect_mapping(value, context)
    expect_exact_keys(
        result,
        {"kind", "schema_version", "outcome"},
        context,
    )
    kind = expect_concrete_string(result.get("kind"), f"{context}.kind")
    if kind != _COMPILER_RESULT_KIND:
        raise ValueError(f"{context}.kind must be {_COMPILER_RESULT_KIND!r}")
    version = expect_concrete_string(
        result.get("schema_version"),
        f"{context}.schema_version",
    )
    if version != SCHEMA_VERSION:
        raise ValueError(f"{context}.schema_version must be {SCHEMA_VERSION!r}")
    return compile_model.CompilerResult(
        outcome=_parse_compile_outcome(
            result.get("outcome"),
            f"{context}.outcome",
        )
    )


def compiler_result_payload(
    result: compile_model.CompilerResult,
) -> dict[str, JsonValue]:
    """Return a compiler result as canonical JSON data.

    Parameters
    ----------
    result : compile_model.CompilerResult
        Compiler result to encode.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical compiler-result payload.
    """
    return {
        "kind": _COMPILER_RESULT_KIND,
        "schema_version": SCHEMA_VERSION,
        "outcome": _compile_outcome_payload(result.outcome),
    }


def _reject_non_json_constant(value: str) -> None:
    """Reject JavaScript numeric constants that are not valid JSON."""
    raise ValueError(f"compiler result contains invalid JSON constant {value!r}")


def _reject_duplicate_json_keys(
    pairs: list[tuple[str, YamlValue]],
) -> dict[str, YamlValue]:
    """Build one JSON object while rejecting ambiguous duplicate keys."""
    result: dict[str, YamlValue] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"compiler result contains duplicate JSON key {key!r}")
        result[key] = value
    return result


def _require_json_depth(value: YamlValue, maximum: int) -> None:
    """Reject JSON containers deeper than the transport's fixed limit."""
    pending: list[tuple[YamlValue, int]] = [(value, 1)]
    while pending:
        item, depth = pending.pop()
        if depth > maximum:
            raise ValueError(
                f"compiler result JSON nesting depth must not exceed {maximum}"
            )
        if isinstance(item, dict):
            pending.extend((child, depth + 1) for child in item.values())
        elif isinstance(item, list):
            pending.extend((child, depth + 1) for child in item)


def parse_compiler_result_bytes(content: bytes) -> compile_model.CompilerResult:
    """Parse exactly one UTF-8 JSON compiler result from raw stdout bytes.

    Parameters
    ----------
    content : bytes
        Raw machine-mode stdout.

    Returns
    -------
    compile_model.CompilerResult
        Validated compiler result.

    Raises
    ------
    TypeError
        ``content`` is not bytes.
    UnicodeDecodeError
        ``content`` is not valid UTF-8.
    ValueError
        The transport exceeds its limits or contains invalid JSON or schema data.
    """
    snapshot = snapshot_bytes(content, "compiler result content")
    if len(snapshot) > MAX_COMPILER_RESULT_BYTES:
        raise ValueError(
            f"compiler result content exceeds {MAX_COMPILER_RESULT_BYTES} bytes"
        )
    try:
        text = snapshot.decode("utf-8")
        value = cast(
            YamlValue,
            json.loads(
                text,
                object_pairs_hook=_reject_duplicate_json_keys,
                parse_constant=_reject_non_json_constant,
            ),
        )
        _require_json_depth(value, MAX_COMPILER_RESULT_JSON_DEPTH)
        return parse_compiler_result(value)
    except (OverflowError, RecursionError) as error:
        raise ValueError("compiler result exceeds JSON structural limits") from error


def parse_process_termination(
    value: YamlValue,
    context: str,
) -> compile_model.ProcessTermination:
    """Parse an exited or signaled compiler-process termination.

    Parameters
    ----------
    value : YamlValue
        Candidate process-termination mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    compile_model.ProcessTermination
        Validated process termination.

    Raises
    ------
    TypeError
        A termination field has the wrong type.
    ValueError
        The termination violates its closed variant schema.
    """
    termination = expect_mapping(value, context)
    kind = expect_concrete_string(termination.get("kind"), f"{context}.kind")
    if kind not in _PROCESS_TERMINATION_KINDS:
        raise ValueError(
            f"{context}.kind must be one of {sorted(_PROCESS_TERMINATION_KINDS)}"
        )
    if kind == "exited":
        expect_exact_keys(termination, {"kind", "code"}, context)
        return compile_model.ExitedProcess(
            code=expect_non_negative_int(
                termination.get("code"),
                f"{context}.code",
            ),
        )
    expect_exact_keys(termination, {"kind", "signal"}, context)
    return compile_model.SignaledProcess(
        signal=expect_positive_int(
            termination.get("signal"),
            f"{context}.signal",
        ),
    )


def process_termination_payload(
    termination: compile_model.ProcessTermination,
) -> dict[str, JsonValue]:
    """Return a discriminated compiler-process termination.

    Parameters
    ----------
    termination : compile_model.ProcessTermination
        Process termination to encode.

    Returns
    -------
    dict[str, JsonValue]
        Canonical termination payload.
    """
    if isinstance(termination, compile_model.ExitedProcess):
        return {
            "kind": "exited",
            "code": termination.code,
        }
    return {
        "kind": "signaled",
        "signal": termination.signal,
    }


def parse_compile_observation(
    value: YamlValue,
    context: str = "compile_observation",
) -> compile_model.CompileObservation:
    """Parse a closed compile-observation artifact.

    Parameters
    ----------
    value : YamlValue
        Candidate compile-observation mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    compile_model.CompileObservation
        Validated compile observation.

    Raises
    ------
    TypeError
        An observation field has the wrong type.
    ValueError
        The observation violates its closed artifact schema.
    """
    artifact = expect_mapping(value, context)
    expect_exact_keys(
        artifact,
        {
            "kind",
            "schema_version",
            "suite_id",
            "test_id",
            "specification_commit",
            "run_id",
            "context_digest",
            "input_digest",
            "compiler_result_digest",
            "log_digest",
            "termination",
            "outcome",
        },
        context,
    )
    kind = expect_concrete_string(artifact.get("kind"), f"{context}.kind")
    if kind != _COMPILE_OBSERVATION_KIND:
        raise ValueError(f"{context}.kind must be {_COMPILE_OBSERVATION_KIND!r}")
    version = expect_concrete_string(
        artifact.get("schema_version"),
        f"{context}.schema_version",
    )
    if version != SCHEMA_VERSION:
        raise ValueError(f"{context}.schema_version must be {SCHEMA_VERSION!r}")
    return compile_model.CompileObservation(
        suite_id=expect_concrete_string(
            artifact.get("suite_id"),
            f"{context}.suite_id",
        ),
        test_id=expect_concrete_string(
            artifact.get("test_id"),
            f"{context}.test_id",
        ),
        specification_commit=expect_git_commit(
            artifact.get("specification_commit"),
            f"{context}.specification_commit",
        ),
        run_id=expect_concrete_string(
            artifact.get("run_id"),
            f"{context}.run_id",
        ),
        context_digest=expect_digest(
            artifact.get("context_digest"),
            f"{context}.context_digest",
        ),
        input_digest=expect_digest(
            artifact.get("input_digest"),
            f"{context}.input_digest",
        ),
        compiler_result_digest=expect_digest(
            artifact.get("compiler_result_digest"),
            f"{context}.compiler_result_digest",
        ),
        log_digest=expect_digest(
            artifact.get("log_digest"),
            f"{context}.log_digest",
        ),
        termination=parse_process_termination(
            artifact.get("termination"),
            f"{context}.termination",
        ),
        outcome=_parse_compile_outcome(
            artifact.get("outcome"),
            f"{context}.outcome",
        ),
    )


def compile_observation_payload(
    observation: compile_model.CompileObservation,
) -> dict[str, JsonValue]:
    """Return a compile observation as canonical JSON data.

    Parameters
    ----------
    observation : compile_model.CompileObservation
        Compile observation to encode.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical compile-observation payload.
    """
    return {
        "kind": _COMPILE_OBSERVATION_KIND,
        "schema_version": SCHEMA_VERSION,
        "suite_id": observation.suite_id,
        "test_id": observation.test_id,
        "specification_commit": observation.specification_commit,
        "run_id": observation.run_id,
        "context_digest": observation.context_digest,
        "input_digest": observation.input_digest,
        "compiler_result_digest": observation.compiler_result_digest,
        "log_digest": observation.log_digest,
        "termination": process_termination_payload(observation.termination),
        "outcome": _compile_outcome_payload(observation.outcome),
    }


def compile_observation_digest(observation: compile_model.CompileObservation) -> str:
    """Return the canonical content address for a compile observation.

    Parameters
    ----------
    observation : compile_model.CompileObservation
        Compile observation to address.

    Returns
    -------
    str
        Lowercase SHA-256 content address with its algorithm prefix.
    """
    return canonical_digest(compile_observation_payload(observation))


def _parse_specification_binding(
    value: YamlValue,
    context: str,
) -> compile_model.SpecificationBinding:
    """Parse one exact specification release/sign-off binding."""
    specification = expect_mapping(value, context)
    expect_exact_keys(
        specification,
        {
            "release",
            "commit",
            "spec_package_signoff_digest",
            "conformance_bundle_digest",
        },
        context,
    )
    release = expect_concrete_string(
        specification.get("release"),
        f"{context}.release",
    )
    release_identity.parse_release_label(release, f"{context}.release")
    return compile_model.SpecificationBinding(
        release=release,
        commit=expect_git_commit(
            specification.get("commit"),
            f"{context}.commit",
        ),
        spec_package_signoff_digest=expect_digest(
            specification.get("spec_package_signoff_digest"),
            f"{context}.spec_package_signoff_digest",
        ),
        conformance_bundle_digest=expect_digest(
            specification.get("conformance_bundle_digest"),
            f"{context}.conformance_bundle_digest",
        ),
    )


def _specification_binding_payload(
    specification: compile_model.SpecificationBinding,
) -> dict[str, JsonValue]:
    """Return one specification binding as canonical JSON data."""
    return {
        "release": specification.release,
        "commit": specification.commit,
        "spec_package_signoff_digest": (specification.spec_package_signoff_digest),
        "conformance_bundle_digest": specification.conformance_bundle_digest,
    }


def parse_compile_evidence_binding(
    value: YamlValue,
    context: str = "compile_evidence_binding",
) -> compile_model.CompileEvidenceBinding:
    """Parse an exact compile-observation set binding.

    Parameters
    ----------
    value : YamlValue
        Candidate compile-evidence binding mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    compile_model.CompileEvidenceBinding
        Validated exact-set binding.

    Raises
    ------
    TypeError
        A binding field has the wrong type.
    ValueError
        The binding violates its closed artifact or exact-set schema.
    """
    artifact = expect_mapping(value, context)
    expect_exact_keys(
        artifact,
        {
            "kind",
            "schema_version",
            "specification",
            "run_id",
            "context_digest",
            "observation_digests",
        },
        context,
    )
    kind = expect_concrete_string(artifact.get("kind"), f"{context}.kind")
    if kind != _COMPILE_EVIDENCE_BINDING_KIND:
        raise ValueError(f"{context}.kind must be {_COMPILE_EVIDENCE_BINDING_KIND!r}")
    version = expect_concrete_string(
        artifact.get("schema_version"),
        f"{context}.schema_version",
    )
    if version != SCHEMA_VERSION:
        raise ValueError(f"{context}.schema_version must be {SCHEMA_VERSION!r}")
    observation_digests = tuple(
        expect_digest(item, f"{context}.observation_digests[{index}]")
        for index, item in enumerate(
            expect_list(
                artifact.get("observation_digests"),
                f"{context}.observation_digests",
            )
        )
    )
    if not observation_digests:
        raise ValueError(f"{context}.observation_digests must not be empty")
    if observation_digests != tuple(sorted(observation_digests)):
        raise ValueError(f"{context}.observation_digests must be sorted")
    if len(set(observation_digests)) != len(observation_digests):
        raise ValueError(f"{context}.observation_digests must not contain duplicates")
    return compile_model.CompileEvidenceBinding(
        specification=_parse_specification_binding(
            artifact.get("specification"),
            f"{context}.specification",
        ),
        run_id=expect_concrete_string(
            artifact.get("run_id"),
            f"{context}.run_id",
        ),
        context_digest=expect_digest(
            artifact.get("context_digest"),
            f"{context}.context_digest",
        ),
        observation_digests=observation_digests,
    )


def compile_evidence_binding_payload(
    binding: compile_model.CompileEvidenceBinding,
) -> dict[str, JsonValue]:
    """Return a compile-evidence binding as canonical JSON data.

    Parameters
    ----------
    binding : compile_model.CompileEvidenceBinding
        Compile-evidence binding to encode.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical binding payload.
    """
    return {
        "kind": _COMPILE_EVIDENCE_BINDING_KIND,
        "schema_version": SCHEMA_VERSION,
        "specification": _specification_binding_payload(binding.specification),
        "run_id": binding.run_id,
        "context_digest": binding.context_digest,
        "observation_digests": list(binding.observation_digests),
    }

"""Decode and address execution evidence artifacts."""

from typing import Literal, cast

from eglur_conformance.artifact_content import (
    SCHEMA_VERSION,
    JsonValue,
    canonical_digest,
    expect_concrete_string,
    expect_digest,
    expect_exact_keys,
    expect_list,
    expect_mapping,
)
from eglur_conformance.evidence.execution import model as execution_model
from eglur_conformance.yaml_document import YamlValue

_RESULT_SHARD_KIND = "eglur_result_shard"
_TEST_RESULT_KIND = "eglur_test_result"
_EVIDENCE_BINDING_KIND = "eglur_implementation_evidence_binding"
_TEST_RESULT_STATUSES = frozenset({"failed", "not_run", "passed", "unsupported"})


def _parse_result_record(
    value: YamlValue, context: str
) -> execution_model.ResultRecord:
    """Parse one explicit test-level context and result binding."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {
            "suite_id",
            "test_id",
            "run_id",
            "context_digest",
            "result_digest",
        },
        context,
    )
    return execution_model.ResultRecord(
        suite_id=expect_concrete_string(
            mapping.get("suite_id"),
            f"{context}.suite_id",
        ),
        test_id=expect_concrete_string(mapping.get("test_id"), f"{context}.test_id"),
        run_id=expect_concrete_string(mapping.get("run_id"), f"{context}.run_id"),
        context_digest=expect_digest(
            mapping.get("context_digest"),
            f"{context}.context_digest",
        ),
        result_digest=expect_digest(
            mapping.get("result_digest"),
            f"{context}.result_digest",
        ),
    )


def parse_result_shard(
    value: YamlValue,
    context: str = "result_shard",
) -> execution_model.ResultShard:
    """Parse a closed result-shard artifact.

    Parameters
    ----------
    value : YamlValue
        Candidate result-shard mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    execution_model.ResultShard
        Validated non-empty result shard.

    Raises
    ------
    TypeError
        A shard field has the wrong type.
    ValueError
        The shard violates its closed artifact schema.
    """
    artifact = expect_mapping(value, context)
    expect_exact_keys(artifact, {"kind", "schema_version", "records"}, context)
    kind = expect_concrete_string(artifact.get("kind"), f"{context}.kind")
    if kind != _RESULT_SHARD_KIND:
        raise ValueError(f"{context}.kind must be {_RESULT_SHARD_KIND!r}")
    version = expect_concrete_string(
        artifact.get("schema_version"),
        f"{context}.schema_version",
    )
    if version != SCHEMA_VERSION:
        raise ValueError(f"{context}.schema_version must be {SCHEMA_VERSION!r}")
    records = tuple(
        _parse_result_record(item, f"{context}.records[{index}]")
        for index, item in enumerate(
            expect_list(artifact.get("records"), f"{context}.records")
        )
    )
    if not records:
        raise ValueError(f"{context}.records must not be empty")
    return execution_model.ResultShard(records)


def parse_test_result(
    value: YamlValue,
    context: str = "test_result",
) -> execution_model.TestResult:
    """Parse a closed test-level execution-state artifact.

    Parameters
    ----------
    value : YamlValue
        Candidate test-result mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    execution_model.TestResult
        Validated executed or not-run result.

    Raises
    ------
    TypeError
        A result field has the wrong type.
    ValueError
        The result violates its closed variant schema.
    """
    artifact = expect_mapping(value, context)
    status = expect_concrete_string(artifact.get("status"), f"{context}.status")
    if status not in _TEST_RESULT_STATUSES:
        raise ValueError(
            f"{context}.status must be one of {sorted(_TEST_RESULT_STATUSES)}"
        )
    common_keys = {
        "kind",
        "schema_version",
        "suite_id",
        "test_id",
        "run_id",
        "context_digest",
        "status",
        "log_digest",
    }
    if status == "not_run":
        expect_exact_keys(artifact, common_keys | {"decision_digest"}, context)
        observed_result_digest = None
        decision_digest = expect_digest(
            artifact.get("decision_digest"),
            f"{context}.decision_digest",
        )
    else:
        expect_exact_keys(
            artifact,
            common_keys | {"observed_result_digest"},
            context,
        )
        observed_result_digest = expect_digest(
            artifact.get("observed_result_digest"),
            f"{context}.observed_result_digest",
        )
        decision_digest = None

    kind = expect_concrete_string(artifact.get("kind"), f"{context}.kind")
    if kind != _TEST_RESULT_KIND:
        raise ValueError(f"{context}.kind must be {_TEST_RESULT_KIND!r}")
    version = expect_concrete_string(
        artifact.get("schema_version"),
        f"{context}.schema_version",
    )
    if version != SCHEMA_VERSION:
        raise ValueError(f"{context}.schema_version must be {SCHEMA_VERSION!r}")
    common = {
        "suite_id": expect_concrete_string(
            artifact.get("suite_id"),
            f"{context}.suite_id",
        ),
        "test_id": expect_concrete_string(
            artifact.get("test_id"),
            f"{context}.test_id",
        ),
        "run_id": expect_concrete_string(
            artifact.get("run_id"),
            f"{context}.run_id",
        ),
        "context_digest": expect_digest(
            artifact.get("context_digest"),
            f"{context}.context_digest",
        ),
        "log_digest": expect_digest(
            artifact.get("log_digest"),
            f"{context}.log_digest",
        ),
    }
    if status == "not_run":
        return execution_model.NotRunTestResult(
            **common,
            decision_digest=cast(str, decision_digest),
        )
    return execution_model.ObservedTestResult(
        **common,
        status=cast(Literal["failed", "passed", "unsupported"], status),
        observed_result_digest=cast(str, observed_result_digest),
    )


def parse_evidence_binding(
    value: YamlValue,
    context: str = "evidence_binding",
) -> execution_model.EvidenceBinding:
    """Parse a closed implementation-evidence sign-off binding.

    Parameters
    ----------
    value : YamlValue
        Candidate evidence-binding mapping.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    execution_model.EvidenceBinding
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
            "run_id",
            "context_digest",
            "result_shard_digests",
        },
        context,
    )
    kind = expect_concrete_string(artifact.get("kind"), f"{context}.kind")
    if kind != _EVIDENCE_BINDING_KIND:
        raise ValueError(f"{context}.kind must be {_EVIDENCE_BINDING_KIND!r}")
    version = expect_concrete_string(
        artifact.get("schema_version"),
        f"{context}.schema_version",
    )
    if version != SCHEMA_VERSION:
        raise ValueError(f"{context}.schema_version must be {SCHEMA_VERSION!r}")
    shard_digests = tuple(
        expect_digest(item, f"{context}.result_shard_digests[{index}]")
        for index, item in enumerate(
            expect_list(
                artifact.get("result_shard_digests"),
                f"{context}.result_shard_digests",
            )
        )
    )
    if not shard_digests:
        raise ValueError(f"{context}.result_shard_digests must not be empty")
    if tuple(sorted(shard_digests)) != shard_digests:
        raise ValueError(f"{context}.result_shard_digests must be sorted")
    if len(set(shard_digests)) != len(shard_digests):
        raise ValueError(f"{context}.result_shard_digests must not contain duplicates")
    return execution_model.EvidenceBinding(
        run_id=expect_concrete_string(
            artifact.get("run_id"),
            f"{context}.run_id",
        ),
        context_digest=expect_digest(
            artifact.get("context_digest"),
            f"{context}.context_digest",
        ),
        result_shard_digests=shard_digests,
    )


def result_shard_payload(shard: execution_model.ResultShard) -> dict[str, JsonValue]:
    """Return a result shard as canonical JSON data.

    Parameters
    ----------
    shard : execution_model.ResultShard
        Result shard to encode.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical result-shard payload.
    """
    return {
        "kind": _RESULT_SHARD_KIND,
        "schema_version": SCHEMA_VERSION,
        "records": [
            {
                "suite_id": record.suite_id,
                "test_id": record.test_id,
                "run_id": record.run_id,
                "context_digest": record.context_digest,
                "result_digest": record.result_digest,
            }
            for record in shard.records
        ],
    }


def test_result_payload(result: execution_model.TestResult) -> dict[str, JsonValue]:
    """Return a test-level result as canonical JSON data.

    Parameters
    ----------
    result : execution_model.TestResult
        Test result to encode.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical test-result payload.
    """
    payload: dict[str, JsonValue] = {
        "kind": _TEST_RESULT_KIND,
        "schema_version": SCHEMA_VERSION,
        "suite_id": result.suite_id,
        "test_id": result.test_id,
        "run_id": result.run_id,
        "context_digest": result.context_digest,
        "status": result.status,
        "log_digest": result.log_digest,
    }
    if isinstance(result, execution_model.NotRunTestResult):
        payload["decision_digest"] = result.decision_digest
    else:
        payload["observed_result_digest"] = result.observed_result_digest
    return payload


def evidence_binding_payload(
    binding: execution_model.EvidenceBinding,
) -> dict[str, JsonValue]:
    """Return an evidence binding as canonical JSON data.

    Parameters
    ----------
    binding : execution_model.EvidenceBinding
        Evidence binding to encode.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical binding payload.
    """
    return {
        "kind": _EVIDENCE_BINDING_KIND,
        "schema_version": SCHEMA_VERSION,
        "run_id": binding.run_id,
        "context_digest": binding.context_digest,
        "result_shard_digests": list(binding.result_shard_digests),
    }


def result_shard_digest(shard: execution_model.ResultShard) -> str:
    """Return the canonical content address for a result shard.

    Parameters
    ----------
    shard : execution_model.ResultShard
        Result shard to address.

    Returns
    -------
    str
        Lowercase SHA-256 content address with its algorithm prefix.
    """
    return canonical_digest(result_shard_payload(shard))


def test_result_digest(result: execution_model.TestResult) -> str:
    """Return the canonical content address for a test-level result.

    Parameters
    ----------
    result : execution_model.TestResult
        Test result to address.

    Returns
    -------
    str
        Lowercase SHA-256 content address with its algorithm prefix.
    """
    return canonical_digest(test_result_payload(result))

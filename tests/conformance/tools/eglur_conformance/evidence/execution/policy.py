"""Validate execution evidence across artifact boundaries."""

from collections.abc import Mapping
from typing import cast

from eglur_conformance.artifact_content import raw_artifact_digest
from eglur_conformance.evidence.execution import codec as execution_codec
from eglur_conformance.evidence.execution import model as execution_model
from eglur_conformance.evidence.execution import projection as execution_projection
from eglur_conformance.evidence.execution_context import codec as context_codec
from eglur_conformance.evidence.execution_context import model as context_model
from eglur_conformance.suite import model as suite_model
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue


def validate_evidence_binding(
    contexts: tuple[context_model.ExecutionContext, ...],
    shards: tuple[execution_model.ResultShard, ...],
    binding: execution_model.EvidenceBinding,
) -> list[ValidationProblem]:
    """Validate an exact, homogeneous run, context, and shard binding.

    Parameters
    ----------
    contexts : tuple[context_model.ExecutionContext, ...]
        Supplied execution-context artifacts.
    shards : tuple[execution_model.ResultShard, ...]
        Supplied result-shard artifacts.
    binding : execution_model.EvidenceBinding
        Exact run, context, and shard-set binding.

    Returns
    -------
    list[ValidationProblem]
        Ordered binding and canonical-model problems.
    """
    problems: list[ValidationProblem] = []
    try:
        normalized_binding = execution_codec.parse_evidence_binding(
            cast(YamlValue, execution_codec.evidence_binding_payload(binding)),
            "binding",
        )
    except (TypeError, ValueError) as error:
        return [ValidationProblem(f"invalid evidence binding: {error}")]
    if normalized_binding != binding:
        return [
            ValidationProblem("invalid evidence binding: direct model is not canonical")
        ]
    binding = normalized_binding

    normalized_contexts: list[context_model.ExecutionContext] = []
    for index, context in enumerate(contexts):
        try:
            normalized = context_codec.parse_execution_context(
                cast(YamlValue, context_codec.execution_context_payload(context)),
                f"contexts[{index}]",
            )
        except (TypeError, ValueError) as error:
            problems.append(
                ValidationProblem(f"invalid execution context artifact: {error}")
            )
            continue
        if normalized != context:
            problems.append(
                ValidationProblem(
                    "invalid execution context artifact: direct model is not canonical"
                )
            )
            continue
        normalized_contexts.append(normalized)

    normalized_shards: list[execution_model.ResultShard] = []
    for index, shard in enumerate(shards):
        try:
            normalized = execution_codec.parse_result_shard(
                cast(YamlValue, execution_codec.result_shard_payload(shard)),
                f"shards[{index}]",
            )
        except (TypeError, ValueError) as error:
            problems.append(
                ValidationProblem(f"invalid result shard artifact: {error}")
            )
            continue
        if normalized != shard:
            problems.append(
                ValidationProblem(
                    "invalid result shard artifact: direct model is not canonical"
                )
            )
            continue
        normalized_shards.append(normalized)

    contexts_by_digest: dict[str, context_model.ExecutionContext] = {}
    for context in normalized_contexts:
        digest = context_codec.execution_context_digest(context)
        if digest in contexts_by_digest:
            problems.append(
                ValidationProblem(f"duplicate execution context artifact: {digest}")
            )
        contexts_by_digest[digest] = context

    if binding.context_digest not in contexts_by_digest:
        problems.append(
            ValidationProblem(
                "binding context digest has no matching execution context artifact: "
                f"{binding.context_digest}"
            )
        )
    extra_contexts = sorted(set(contexts_by_digest) - {binding.context_digest})
    for digest in extra_contexts:
        problems.append(
            ValidationProblem(
                f"unbound execution context artifact is incompatible: {digest}"
            )
        )

    shards_by_digest: dict[str, execution_model.ResultShard] = {}
    for shard in normalized_shards:
        digest = execution_codec.result_shard_digest(shard)
        if digest in shards_by_digest:
            problems.append(ValidationProblem(f"duplicate result shard: {digest}"))
        shards_by_digest[digest] = shard

    expected_shards = set(binding.result_shard_digests)
    actual_shards = set(shards_by_digest)
    for digest in sorted(expected_shards - actual_shards):
        problems.append(
            ValidationProblem(f"bound result shard artifact is missing: {digest}")
        )
    for digest in sorted(actual_shards - expected_shards):
        problems.append(
            ValidationProblem(f"result shard artifact is not bound: {digest}")
        )

    seen_records: set[suite_model.TestIdentity] = set()
    for shard_digest, shard in sorted(shards_by_digest.items()):
        for record in shard.records:
            record_key = record.identity
            if record_key in seen_records:
                problems.append(
                    ValidationProblem(
                        "duplicate test-level result record across shards: "
                        f"{record_key}"
                    )
                )
            seen_records.add(record_key)
            if record.run_id != binding.run_id:
                problems.append(
                    ValidationProblem(
                        f"result shard {shard_digest} record "
                        f"{record_key} has run_id "
                        f"{record.run_id!r}, expected {binding.run_id!r}"
                    )
                )
            if record.context_digest != binding.context_digest:
                problems.append(
                    ValidationProblem(
                        f"result shard {shard_digest} record "
                        f"{record_key} has context "
                        f"{record.context_digest}, expected {binding.context_digest}"
                    )
                )

    return problems


def _normalize_test_results(
    results: tuple[execution_model.TestResult, ...],
) -> tuple[list[execution_model.TestResult], list[ValidationProblem]]:
    """Reparse public test-result models before semantic validation."""
    problems: list[ValidationProblem] = []
    normalized_results: list[execution_model.TestResult] = []
    for index, result in enumerate(results):
        try:
            normalized = execution_codec.parse_test_result(
                cast(YamlValue, execution_codec.test_result_payload(result)),
                f"results[{index}]",
            )
        except (TypeError, ValueError) as error:
            problems.append(ValidationProblem(f"invalid test result artifact: {error}"))
            continue
        if normalized != result:
            problems.append(
                ValidationProblem(
                    "invalid test result artifact: direct model is not canonical"
                )
            )
            continue
        normalized_results.append(normalized)
    return normalized_results, problems


def validate_test_result_binding(
    results: tuple[execution_model.TestResult, ...],
    shards: tuple[execution_model.ResultShard, ...],
) -> list[ValidationProblem]:
    """Bind every shard record to one exact structured test-result artifact.

    Parameters
    ----------
    results : tuple[execution_model.TestResult, ...]
        Supplied structured test-result artifacts.
    shards : tuple[execution_model.ResultShard, ...]
        Supplied result-shard artifacts.

    Returns
    -------
    list[ValidationProblem]
        Ordered exact-set and record-identity problems.
    """
    normalized_results, problems = _normalize_test_results(results)

    normalized_shards: list[execution_model.ResultShard] = []
    for index, shard in enumerate(shards):
        try:
            normalized = execution_codec.parse_result_shard(
                cast(YamlValue, execution_codec.result_shard_payload(shard)),
                f"shards[{index}]",
            )
        except (TypeError, ValueError) as error:
            problems.append(
                ValidationProblem(f"invalid result shard artifact: {error}")
            )
            continue
        if normalized != shard:
            problems.append(
                ValidationProblem(
                    "invalid result shard artifact: direct model is not canonical"
                )
            )
            continue
        normalized_shards.append(normalized)

    results_by_digest: dict[str, execution_model.TestResult] = {}
    seen_result_ids: set[suite_model.TestIdentity] = set()
    for result in normalized_results:
        digest = execution_codec.test_result_digest(result)
        if digest in results_by_digest:
            problems.append(
                ValidationProblem(f"duplicate test result artifact: {digest}")
            )
        results_by_digest[digest] = result
        result_id = result.identity
        if result_id in seen_result_ids:
            problems.append(
                ValidationProblem(f"duplicate test result identity: {result_id}")
            )
        seen_result_ids.add(result_id)

    records_by_digest: dict[str, execution_model.ResultRecord] = {}
    for shard in normalized_shards:
        for record in shard.records:
            if record.result_digest in records_by_digest:
                problems.append(
                    ValidationProblem(
                        "duplicate result artifact reference across shards: "
                        f"{record.result_digest}"
                    )
                )
            records_by_digest[record.result_digest] = record

    result_digests = set(results_by_digest)
    record_digests = set(records_by_digest)
    for digest in sorted(record_digests - result_digests):
        problems.append(
            ValidationProblem(f"referenced test result artifact is missing: {digest}")
        )
    for digest in sorted(result_digests - record_digests):
        problems.append(
            ValidationProblem(f"test result artifact is not shard-bound: {digest}")
        )

    for digest in sorted(result_digests & record_digests):
        result = results_by_digest[digest]
        record = records_by_digest[digest]
        expected_identity = (
            result.suite_id,
            result.test_id,
            result.run_id,
            result.context_digest,
        )
        record_identity = (
            record.suite_id,
            record.test_id,
            record.run_id,
            record.context_digest,
        )
        if record_identity != expected_identity:
            problems.append(
                ValidationProblem(
                    f"result shard identity for {digest} is {record_identity}, "
                    f"expected {expected_identity}"
                )
            )

    return problems


def validate_raw_artifact_binding(
    results: tuple[execution_model.TestResult, ...],
    raw_artifacts: tuple[bytes, ...],
) -> list[ValidationProblem]:
    """Bind every declared raw-artifact digest to the exact supplied bytes.

    Parameters
    ----------
    results : tuple[execution_model.TestResult, ...]
        Structured results declaring raw-artifact addresses.
    raw_artifacts : tuple[bytes, ...]
        Supplied observed-result, decision, and log bytes.

    Returns
    -------
    list[ValidationProblem]
        Ordered missing, duplicate, and unbound artifact problems.
    """
    normalized_results, problems = _normalize_test_results(results)
    required_digests: set[str] = set()
    for result in normalized_results:
        required_digests.add(result.log_digest)
        if isinstance(result, execution_model.NotRunTestResult):
            required_digests.add(result.decision_digest)
        else:
            required_digests.add(result.observed_result_digest)

    supplied_by_digest: dict[str, bytes] = {}
    for content in raw_artifacts:
        digest = raw_artifact_digest(content)
        if digest in supplied_by_digest:
            problems.append(
                ValidationProblem(f"duplicate raw artifact content: {digest}")
            )
        supplied_by_digest[digest] = content

    supplied_digests = set(supplied_by_digest)
    for digest in sorted(required_digests - supplied_digests):
        problems.append(ValidationProblem(f"raw artifact is missing: {digest}"))
    for digest in sorted(supplied_digests - required_digests):
        problems.append(ValidationProblem(f"raw artifact is not bound: {digest}"))
    return problems


def validate_optional_probe_evidence(
    contexts: tuple[context_model.ExecutionContext, ...],
    results: tuple[execution_model.TestResult, ...],
    shards: tuple[execution_model.ResultShard, ...],
    raw_artifacts: tuple[bytes, ...],
    binding: execution_model.EvidenceBinding,
    *,
    manifest: YamlValue,
    suite_documents: Mapping[str, YamlValue],
) -> list[ValidationProblem]:
    """Validate exact optional-probe records inside one implementation run.

    Parameters
    ----------
    contexts : tuple[context_model.ExecutionContext, ...]
        Supplied execution-context artifacts.
    results : tuple[execution_model.TestResult, ...]
        Supplied structured test-result artifacts.
    shards : tuple[execution_model.ResultShard, ...]
        Supplied result-shard artifacts.
    raw_artifacts : tuple[bytes, ...]
        Supplied raw result, decision, and log bytes.
    binding : execution_model.EvidenceBinding
        Exact run, context, and shard-set binding.
    manifest : YamlValue
        Parsed conformance manifest.
    suite_documents : Mapping[str, YamlValue]
        Parsed suite documents keyed by manifest path.

    Returns
    -------
    list[ValidationProblem]
        Ordered binding, inventory, and optional-probe coverage problems.
    """
    problems = [
        *validate_evidence_binding(contexts, shards, binding),
        *validate_test_result_binding(results, shards),
        *validate_raw_artifact_binding(results, raw_artifacts),
    ]
    try:
        inventory = execution_projection.parse_conformance_test_inventory(
            manifest, suite_documents
        )
    except (TypeError, ValueError) as error:
        problems.append(ValidationProblem(f"invalid conformance inventory: {error}"))
        return problems

    if not inventory.optional_probe_test_identities:
        problems.append(
            ValidationProblem("optional probe test ID set must not be empty")
        )
        return problems

    normalized_results, _ = _normalize_test_results(results)
    actual_test_identities = {result.identity for result in normalized_results}
    for identity in sorted(actual_test_identities - inventory.known_test_identities):
        problems.append(ValidationProblem(f"unknown test result identity: {identity}"))
    for identity in sorted(
        inventory.optional_probe_test_identities - actual_test_identities
    ):
        problems.append(ValidationProblem(f"missing optional probe result: {identity}"))
    return problems

"""Validate compilation evidence across artifact boundaries."""

from collections.abc import Mapping
from typing import cast

import yaml

from eglur_conformance.artifact_content import (
    expect_concrete_string,
    expect_digest,
    expect_exact_keys,
    expect_mapping,
    raw_artifact_digest,
    snapshot_bytes,
)
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.evidence.compilation import codec as compile_codec
from eglur_conformance.evidence.compilation import model as compile_model
from eglur_conformance.evidence.compilation import projection as compile_projection
from eglur_conformance.evidence.execution_context import codec as context_codec
from eglur_conformance.evidence.execution_context import model as context_model
from eglur_conformance.suite import artifact as suite_artifact
from eglur_conformance.suite import model as suite_model
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue, load_yaml_text


def _normalize_compile_observations(
    observations: tuple[compile_model.CompileObservation, ...],
) -> tuple[list[compile_model.CompileObservation], list[ValidationProblem]]:
    """Reparse public observation models before semantic validation."""
    normalized: list[compile_model.CompileObservation] = []
    problems: list[ValidationProblem] = []
    for index, observation in enumerate(observations):
        try:
            parsed = compile_codec.parse_compile_observation(
                cast(YamlValue, compile_codec.compile_observation_payload(observation)),
                f"observations[{index}]",
            )
        except (AttributeError, TypeError, ValueError) as error:
            problems.append(ValidationProblem(f"invalid compile observation: {error}"))
            continue
        if parsed != observation:
            problems.append(
                ValidationProblem(
                    "invalid compile observation: direct model is not canonical"
                )
            )
            continue
        normalized.append(parsed)
    return normalized, problems


def validate_spec_package_signoff_binding(
    content: bytes,
    specification: compile_model.SpecificationBinding,
) -> list[ValidationProblem]:
    """Bind exact sign-off bytes and their released specification identity.

    Parameters
    ----------
    content : bytes
        Raw spec-package sign-off artifact.
    specification : compile_model.SpecificationBinding
        Expected release, commit, and artifact addresses.

    Returns
    -------
    list[ValidationProblem]
        Ordered binding and schema problems.
    """
    problems: list[ValidationProblem] = []
    try:
        snapshot = snapshot_bytes(content, "spec-package sign-off content")
        if raw_artifact_digest(snapshot) != specification.spec_package_signoff_digest:
            problems.append(
                ValidationProblem(
                    "spec-package sign-off bytes do not match the bound digest"
                )
            )
        document = load_yaml_text(snapshot.decode("utf-8"))
        signoff = expect_mapping(document, "spec_package_signoff")
        kind = expect_concrete_string(
            signoff.get("kind"),
            "spec_package_signoff.kind",
        )
        if kind != "spec_package_release":
            raise ValueError("spec_package_signoff.kind must be 'spec_package_release'")
        if signoff.get("verdict") != "GO":
            raise ValueError("spec_package_signoff.verdict must be 'GO'")
        if signoff.get("implementation_conformance") != "not_evaluated":
            raise ValueError(
                "spec_package_signoff.implementation_conformance "
                "must be 'not_evaluated'"
            )
        identity = expect_mapping(
            signoff.get("specification"),
            "spec_package_signoff.specification",
        )
        expect_exact_keys(
            identity,
            {"release", "commit"},
            "spec_package_signoff.specification",
        )
        release = expect_concrete_string(
            identity.get("release"),
            "spec_package_signoff.specification.release",
        )
        commit = compile_codec.expect_git_commit(
            identity.get("commit"),
            "spec_package_signoff.specification.commit",
        )
        signed_bundle_digest = expect_digest(
            signoff.get("conformance_bundle_digest"),
            "spec_package_signoff.conformance_bundle_digest",
        )
        if release != specification.release:
            problems.append(
                ValidationProblem(
                    f"spec-package sign-off release is {release}, "
                    f"expected {specification.release}"
                )
            )
        if commit != specification.commit:
            problems.append(
                ValidationProblem(
                    f"spec-package sign-off commit is {commit}, "
                    f"expected {specification.commit}"
                )
            )
        if signed_bundle_digest != specification.conformance_bundle_digest:
            problems.append(
                ValidationProblem(
                    "spec-package sign-off conformance bundle digest is "
                    f"{signed_bundle_digest}, expected "
                    f"{specification.conformance_bundle_digest}"
                )
            )
    except (
        AttributeError,
        UnicodeDecodeError,
        TypeError,
        ValueError,
        yaml.YAMLError,
    ) as error:
        problems.append(
            ValidationProblem(f"invalid spec-package sign-off artifact: {error}")
        )
    return problems


def _validate_raw_compile_artifacts(
    observations: tuple[compile_model.CompileObservation, ...],
    raw_artifacts: tuple[bytes, ...],
) -> list[ValidationProblem]:
    """Bind raw streams and require JSON/result/termination agreement."""
    required = {
        digest
        for observation in observations
        for digest in (
            observation.compiler_result_digest,
            observation.log_digest,
        )
    }
    supplied: dict[str, bytes] = {}
    problems: list[ValidationProblem] = []
    for index, content in enumerate(raw_artifacts):
        try:
            snapshot = snapshot_bytes(
                content,
                f"raw compile artifact[{index}]",
            )
            digest = raw_artifact_digest(snapshot)
        except TypeError as error:
            problems.append(
                ValidationProblem(f"invalid raw compile artifact[{index}]: {error}")
            )
            continue
        if digest in supplied:
            problems.append(
                ValidationProblem(f"duplicate raw compile artifact: {digest}")
            )
        supplied[digest] = snapshot
    supplied_digests = set(supplied)
    for digest in sorted(required - supplied_digests):
        problems.append(ValidationProblem(f"raw compile artifact is missing: {digest}"))
    for digest in sorted(supplied_digests - required):
        problems.append(
            ValidationProblem(f"raw compile artifact is not bound: {digest}")
        )

    compiler_results: dict[str, compile_model.CompilerResult] = {}
    for digest in sorted(
        {
            observation.compiler_result_digest
            for observation in observations
            if observation.compiler_result_digest in supplied
        }
    ):
        try:
            compiler_results[digest] = compile_codec.parse_compiler_result_bytes(
                supplied[digest]
            )
        except (UnicodeDecodeError, TypeError, ValueError) as error:
            problems.append(
                ValidationProblem(
                    f"invalid compiler result stdout artifact {digest}: {error}"
                )
            )

    for observation in observations:
        label = str(observation.identity)
        compiler_result = compiler_results.get(observation.compiler_result_digest)
        if (
            compiler_result is not None
            and compiler_result.outcome != observation.outcome
        ):
            problems.append(
                ValidationProblem(
                    f"{label} compiler result outcome does not match "
                    "compile observation"
                )
            )

        termination = observation.termination
        if isinstance(termination, compile_model.SignaledProcess):
            problems.append(
                ValidationProblem(
                    f"{label} compiler process terminated by signal "
                    f"{termination.signal}"
                )
            )
            continue
        expected_code = 0 if observation.outcome.kind == "succeeded" else 1
        if termination.code != expected_code:
            problems.append(
                ValidationProblem(
                    f"{label} compiler process exit code is {termination.code}, "
                    f"expected {expected_code} for {observation.outcome.kind}"
                )
            )
    return problems


def _validate_diagnostic_match(
    expected: compile_projection.ExpectedDiagnostic,
    observed: compile_model.ObservedDiagnostic,
    identity: suite_model.TestIdentity,
) -> list[ValidationProblem]:
    """Compare one observed primary diagnostic by portable field contract."""
    label = str(identity)
    problems: list[ValidationProblem] = []
    if observed.code != expected.code:
        problems.append(
            ValidationProblem(
                f"{label} primary diagnostic code is {observed.code}, "
                f"expected {expected.code}"
            )
        )
    for token in expected.message_all_of:
        if token not in observed.message:
            problems.append(
                ValidationProblem(
                    f"{label} primary diagnostic message is missing {token!r}"
                )
            )
    for group in expected.message_any_of:
        if not any(token in observed.message for token in group):
            problems.append(
                ValidationProblem(
                    f"{label} primary diagnostic message matches none of "
                    f"{list(group)!r}"
                )
            )
    observed_data = dict(observed.data)
    missing_data_keys = sorted(set(expected.data_keys) - set(observed_data))
    if missing_data_keys:
        problems.append(
            ValidationProblem(
                f"{label} primary diagnostic data is missing keys {missing_data_keys}"
            )
        )
    for key, expected_value in expected.data_equals:
        actual_value = observed_data.get(key)
        if actual_value != expected_value:
            problems.append(
                ValidationProblem(
                    f"{label} primary diagnostic data.{key} is "
                    f"{actual_value!r}, expected {expected_value!r}"
                )
            )
    expected_span = expected.primary_span
    observed_span = observed.primary_span
    if isinstance(expected_span, suite_model.MarkerPrimarySpan):
        actual = (
            (observed_span.source, observed_span.text_range.start.line)
            if isinstance(observed_span, diagnostic_model.SourceSpan)
            and observed_span.text_range is not None
            else (None, None)
        )
        expected_location = (expected_span.source, expected_span.line)
        if actual != expected_location:
            problems.append(
                ValidationProblem(
                    f"{label} primary diagnostic location is {actual}, "
                    f"expected {expected_location}"
                )
            )
    elif isinstance(expected_span, suite_model.BytePrimarySpan):
        actual = (
            (observed_span.source, observed_span.byte_range)
            if isinstance(observed_span, diagnostic_model.SourceSpan)
            else (None, None)
        )
        expected_location = (expected_span.source, expected_span.byte_range)
        if actual != expected_location:
            problems.append(
                ValidationProblem(
                    f"{label} primary diagnostic byte span is {actual}, "
                    f"expected {expected_location}"
                )
            )
    if (
        observed.code == "LEX_INVALID_UTF8"
        and isinstance(observed_span, diagnostic_model.SourceSpan)
        and observed_span.text_range is not None
    ):
        problems.append(
            ValidationProblem(
                f"{label} invalid UTF-8 diagnostic must not include text coordinates"
            )
        )
    return problems


def validate_compile_evidence(
    contexts: tuple[context_model.ExecutionContext, ...],
    observations: tuple[compile_model.CompileObservation, ...],
    raw_artifacts: tuple[bytes, ...],
    binding: compile_model.CompileEvidenceBinding,
    *,
    spec_package_signoff_bytes: bytes,
    manifest_bytes: bytes,
    suite_document_bytes: Mapping[str, bytes],
) -> list[ValidationProblem]:
    """Validate exact per-fixture compiler observations for one implementation.

    Parameters
    ----------
    contexts : tuple[context_model.ExecutionContext, ...]
        Supplied execution-context artifacts.
    observations : tuple[compile_model.CompileObservation, ...]
        Supplied per-fixture compile observations.
    raw_artifacts : tuple[bytes, ...]
        Supplied compiler-result and log bytes.
    binding : compile_model.CompileEvidenceBinding
        Exact specification, run, context, and observation binding.
    spec_package_signoff_bytes : bytes
        Raw spec-package sign-off artifact.
    manifest_bytes : bytes
        Exact conformance manifest bytes.
    suite_document_bytes : Mapping[str, bytes]
        Exact suite-document bytes keyed by manifest path.

    Returns
    -------
    list[ValidationProblem]
        Ordered cross-artifact validation problems.
    """
    ingress_types = (
        ("contexts", contexts, tuple),
        ("observations", observations, tuple),
        ("raw_artifacts", raw_artifacts, tuple),
        ("spec_package_signoff_bytes", spec_package_signoff_bytes, bytes),
        ("manifest_bytes", manifest_bytes, bytes),
        ("suite_document_bytes", suite_document_bytes, Mapping),
    )
    ingress_problems = [
        ValidationProblem(f"{name} must be {expected.__name__}")
        for name, value, expected in ingress_types
        if not isinstance(value, expected)
    ]
    if ingress_problems:
        return ingress_problems

    problems: list[ValidationProblem] = []
    try:
        normalized_binding = compile_codec.parse_compile_evidence_binding(
            cast(YamlValue, compile_codec.compile_evidence_binding_payload(binding)),
            "binding",
        )
    except (AttributeError, TypeError, ValueError) as error:
        return [ValidationProblem(f"invalid compile evidence binding: {error}")]
    if normalized_binding != binding:
        return [
            ValidationProblem(
                "invalid compile evidence binding: direct model is not canonical"
            )
        ]
    binding = normalized_binding
    problems.extend(
        validate_spec_package_signoff_binding(
            spec_package_signoff_bytes,
            binding.specification,
        )
    )

    normalized_contexts: list[context_model.ExecutionContext] = []
    for index, context in enumerate(contexts):
        try:
            parsed = context_codec.parse_execution_context(
                cast(YamlValue, context_codec.execution_context_payload(context)),
                f"contexts[{index}]",
            )
        except (AttributeError, TypeError, ValueError) as error:
            problems.append(
                ValidationProblem(f"invalid execution context artifact: {error}")
            )
            continue
        if parsed != context:
            problems.append(
                ValidationProblem(
                    "invalid execution context artifact: direct model is not canonical"
                )
            )
            continue
        normalized_contexts.append(parsed)
    contexts_by_digest = {
        context_codec.execution_context_digest(context): context
        for context in normalized_contexts
    }
    if len(contexts_by_digest) != len(normalized_contexts):
        problems.append(ValidationProblem("duplicate execution context artifact"))
    selected_context = contexts_by_digest.get(binding.context_digest)
    if selected_context is None:
        problems.append(
            ValidationProblem(
                "compile binding context digest has no matching "
                "execution context artifact"
            )
        )
    for digest in sorted(set(contexts_by_digest) - {binding.context_digest}):
        problems.append(
            ValidationProblem(
                f"unbound execution context artifact is incompatible: {digest}"
            )
        )

    normalized_observations, observation_problems = _normalize_compile_observations(
        observations
    )
    problems.extend(observation_problems)
    observations_by_digest: dict[str, compile_model.CompileObservation] = {}
    observations_by_id: dict[
        suite_model.TestIdentity, compile_model.CompileObservation
    ] = {}
    for observation in normalized_observations:
        digest = compile_codec.compile_observation_digest(observation)
        if digest in observations_by_digest:
            problems.append(
                ValidationProblem(f"duplicate compile observation: {digest}")
            )
        observations_by_digest[digest] = observation
        identity = observation.identity
        if identity in observations_by_id:
            problems.append(
                ValidationProblem(f"duplicate compile observation identity: {identity}")
            )
        observations_by_id[identity] = observation

        if observation.specification_commit != binding.specification.commit:
            problems.append(
                ValidationProblem(
                    f"{identity} specification commit is "
                    f"{observation.specification_commit}, expected "
                    f"{binding.specification.commit}"
                )
            )
        if observation.run_id != binding.run_id:
            problems.append(
                ValidationProblem(
                    f"{identity} run_id is "
                    f"{observation.run_id!r}, expected {binding.run_id!r}"
                )
            )
        if observation.context_digest != binding.context_digest:
            problems.append(
                ValidationProblem(
                    f"{identity} context digest is "
                    f"{observation.context_digest}, expected "
                    f"{binding.context_digest}"
                )
            )

    expected_observation_digests = set(binding.observation_digests)
    actual_observation_digests = set(observations_by_digest)
    for digest in sorted(expected_observation_digests - actual_observation_digests):
        problems.append(
            ValidationProblem(f"bound compile observation is missing: {digest}")
        )
    for digest in sorted(actual_observation_digests - expected_observation_digests):
        problems.append(
            ValidationProblem(f"compile observation is not bound: {digest}")
        )
    problems.extend(
        _validate_raw_compile_artifacts(
            tuple(normalized_observations),
            raw_artifacts,
        )
    )

    try:
        suite_document_snapshot = suite_artifact.snapshot_suite_document_bytes(
            suite_document_bytes
        )
        actual_bundle_digest = suite_artifact.conformance_bundle_digest(
            manifest_bytes,
            suite_document_snapshot,
        )
        conformance_bundle = suite_artifact.parse_conformance_bundle_bytes(
            manifest_bytes,
            suite_document_snapshot,
        )
        inventory = compile_projection.conformance_compile_inventory(conformance_bundle)
    except (
        AttributeError,
        UnicodeDecodeError,
        TypeError,
        ValueError,
        yaml.YAMLError,
    ) as error:
        problems.append(
            ValidationProblem(f"invalid compile fixture inventory: {error}")
        )
        return problems
    if actual_bundle_digest != binding.specification.conformance_bundle_digest:
        problems.append(
            ValidationProblem(
                "conformance manifest/suite bytes do not match the bound bundle digest"
            )
        )
    if inventory.release != binding.specification.release:
        problems.append(
            ValidationProblem(
                f"compile manifest release is {inventory.release}, "
                f"expected {binding.specification.release}"
            )
        )
    if selected_context is None:
        return problems

    active_fixtures = {
        fixture.identity: fixture
        for fixture in inventory.fixtures
        if (
            fixture.precondition is None
            or fixture.precondition in selected_context.active_preconditions
        )
    }
    actual_ids = set(observations_by_id)
    expected_ids = set(active_fixtures)
    for identity in sorted(expected_ids - actual_ids):
        problems.append(
            ValidationProblem(f"required compile observation is missing: {identity}")
        )
    for identity in sorted(actual_ids - expected_ids):
        problems.append(
            ValidationProblem(
                f"compile observation is not an active required fixture: {identity}"
            )
        )

    for identity in sorted(expected_ids & actual_ids):
        fixture = active_fixtures[identity]
        observation = observations_by_id[identity]
        if observation.input_digest != fixture.input_digest:
            problems.append(
                ValidationProblem(
                    f"{identity} input digest is "
                    f"{observation.input_digest}, expected {fixture.input_digest}"
                )
            )
        expected_diagnostic = fixture.expected_diagnostic
        if expected_diagnostic is None:
            if not isinstance(observation.outcome, compile_model.CompileSucceeded):
                problems.append(
                    ValidationProblem(
                        f"{identity} positive fixture "
                        f"was {observation.outcome.kind}, expected succeeded"
                    )
                )
            continue
        if not isinstance(observation.outcome, compile_model.CompileRejected):
            problems.append(
                ValidationProblem(
                    f"{identity} negative fixture "
                    f"was {observation.outcome.kind}, expected rejected"
                )
            )
            continue
        problems.extend(
            _validate_diagnostic_match(
                expected_diagnostic,
                observation.outcome.primary_diagnostic,
                identity,
            )
        )
    return problems

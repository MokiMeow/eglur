"""Decode and address execution-context evidence artifacts."""

import re

from eglur_conformance.artifact_content import (
    SCHEMA_VERSION,
    JsonValue,
    canonical_digest,
    expect_concrete_string,
    expect_exact_keys,
    expect_list,
    expect_mapping,
    expect_non_negative_int,
    expect_positive_int,
)
from eglur_conformance.evidence.execution_context import model as context_model
from eglur_conformance.preconditions import V01_PRECONDITIONS
from eglur_conformance.yaml_document import YamlValue

_EXECUTION_CONTEXT_KIND = "eglur_execution_context"
_DETERMINISM_PROFILES = frozenset({"default", "strict"})
_APPLICABILITY_VALUES = frozenset({"identified", "not_applicable"})
_RANDOMNESS_MODES = frozenset({"unused", "fixed_seed", "system_entropy"})
_REVISION_PATTERN = re.compile(
    r"(?:git:(?:[0-9a-f]{40}|[0-9a-f]{64})|sha256:[0-9a-f]{64})\Z"
)


def _expect_revision(value: YamlValue, context: str) -> str:
    """Return a full Git object ID or SHA-256 content address."""
    revision = expect_concrete_string(value, context)
    if _REVISION_PATTERN.fullmatch(revision) is None:
        raise ValueError(
            f"{context} must be git:<40-or-64 lowercase hex digits> "
            "or sha256:<64 lowercase hex digits>"
        )
    if revision.startswith("git:") and set(revision.removeprefix("git:")) == {"0"}:
        raise ValueError(f"{context} must not be a Git null object ID")
    return revision


def _parse_revisioned_component(
    value: YamlValue,
    context: str,
) -> context_model.RevisionedComponent:
    """Parse one compiler or runtime identity."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"name", "version", "revision"}, context)
    return context_model.RevisionedComponent(
        name=expect_concrete_string(mapping.get("name"), f"{context}.name"),
        version=expect_concrete_string(
            mapping.get("version"),
            f"{context}.version",
        ),
        revision=_expect_revision(
            mapping.get("revision"),
            f"{context}.revision",
        ),
    )


def _parse_target(value: YamlValue, context: str) -> context_model.Target:
    """Parse one compilation target."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"triple"}, context)
    return context_model.Target(
        triple=expect_concrete_string(mapping.get("triple"), f"{context}.triple"),
    )


def _parse_versioned_component(
    value: YamlValue,
    context: str,
) -> context_model.VersionedComponent:
    """Parse an identified or explicitly inapplicable component."""
    mapping = expect_mapping(value, context)
    applicability = expect_concrete_string(
        mapping.get("applicability"),
        f"{context}.applicability",
    )
    if applicability not in _APPLICABILITY_VALUES:
        raise ValueError(
            f"{context}.applicability must be one of {sorted(_APPLICABILITY_VALUES)}"
        )
    if applicability == "not_applicable":
        expect_exact_keys(mapping, {"applicability"}, context)
        return context_model.NotApplicableComponent()

    expect_exact_keys(
        mapping,
        {"applicability", "name", "version", "revision"},
        context,
    )
    return context_model.RevisionedComponent(
        name=expect_concrete_string(mapping.get("name"), f"{context}.name"),
        version=expect_concrete_string(mapping.get("version"), f"{context}.version"),
        revision=_expect_revision(mapping.get("revision"), f"{context}.revision"),
    )


def _parse_device(value: YamlValue, context: str) -> context_model.Device:
    """Parse an identified or explicitly inapplicable device."""
    mapping = expect_mapping(value, context)
    applicability = expect_concrete_string(
        mapping.get("applicability"),
        f"{context}.applicability",
    )
    if applicability not in _APPLICABILITY_VALUES:
        raise ValueError(
            f"{context}.applicability must be one of {sorted(_APPLICABILITY_VALUES)}"
        )
    if applicability == "not_applicable":
        expect_exact_keys(mapping, {"applicability"}, context)
        return context_model.NotApplicableDevice()

    expect_exact_keys(
        mapping,
        {"applicability", "kind", "vendor", "model", "ordinal"},
        context,
    )
    return context_model.IdentifiedDevice(
        kind=expect_concrete_string(mapping.get("kind"), f"{context}.kind"),
        vendor=expect_concrete_string(mapping.get("vendor"), f"{context}.vendor"),
        model=expect_concrete_string(mapping.get("model"), f"{context}.model"),
        ordinal=expect_non_negative_int(mapping.get("ordinal"), f"{context}.ordinal"),
    )


def _parse_randomness(value: YamlValue, context: str) -> context_model.Randomness:
    """Parse one closed randomness configuration variant."""
    mapping = expect_mapping(value, context)
    mode = expect_concrete_string(mapping.get("mode"), f"{context}.mode")
    if mode not in _RANDOMNESS_MODES:
        raise ValueError(f"{context}.mode must be one of {sorted(_RANDOMNESS_MODES)}")
    if mode == "unused":
        expect_exact_keys(mapping, {"mode"}, context)
        return context_model.UnusedRandomness()
    if mode == "fixed_seed":
        expect_exact_keys(mapping, {"mode", "algorithm", "seed"}, context)
        return context_model.FixedSeedRandomness(
            algorithm=expect_concrete_string(
                mapping.get("algorithm"),
                f"{context}.algorithm",
            ),
            seed=expect_concrete_string(mapping.get("seed"), f"{context}.seed"),
        )

    expect_exact_keys(mapping, {"mode", "source"}, context)
    return context_model.SystemEntropyRandomness(
        source=expect_concrete_string(mapping.get("source"), f"{context}.source"),
    )


def _parse_determinism(value: YamlValue, context: str) -> context_model.Determinism:
    """Parse the default profile and randomness configuration."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"default_profile", "randomness"}, context)
    default_profile = expect_concrete_string(
        mapping.get("default_profile"),
        f"{context}.default_profile",
    )
    if default_profile not in _DETERMINISM_PROFILES:
        raise ValueError(
            f"{context}.default_profile must be one of {sorted(_DETERMINISM_PROFILES)}"
        )
    return context_model.Determinism(
        default_profile,
        _parse_randomness(mapping.get("randomness"), f"{context}.randomness"),
    )


def _parse_parallelism(value: YamlValue, context: str) -> context_model.Parallelism:
    """Parse resolved parallel execution counts."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"workers", "threads", "processes"}, context)
    return context_model.Parallelism(
        workers=expect_positive_int(mapping.get("workers"), f"{context}.workers"),
        threads=expect_positive_int(mapping.get("threads"), f"{context}.threads"),
        processes=expect_positive_int(
            mapping.get("processes"),
            f"{context}.processes",
        ),
    )


def _parse_preconditions(value: YamlValue, context: str) -> tuple[str, ...]:
    """Parse the sorted, duplicate-free closed v0.1 precondition set."""
    preconditions = tuple(
        expect_concrete_string(item, f"{context}[{index}]")
        for index, item in enumerate(expect_list(value, context))
    )
    if tuple(sorted(preconditions)) != preconditions:
        raise ValueError(f"{context} must be sorted")
    if len(set(preconditions)) != len(preconditions):
        raise ValueError(f"{context} must not contain duplicates")
    unknown = sorted(set(preconditions) - V01_PRECONDITIONS)
    if unknown:
        raise ValueError(f"{context} contains unknown v0.1 values: {unknown}")
    return preconditions


def parse_execution_context(
    value: YamlValue,
    context: str = "execution_context",
) -> context_model.ExecutionContext:
    """Parse one closed execution-context artifact.

    Parameters
    ----------
    value : YamlValue
        YAML value containing the artifact.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    context_model.ExecutionContext
        Parsed execution context.

    Raises
    ------
    TypeError
        The artifact contains a value of the wrong scalar or container type.
    ValueError
        The artifact violates the execution-context schema.
    """
    artifact = expect_mapping(value, context)
    expect_exact_keys(artifact, {"kind", "schema_version", "context"}, context)
    kind = expect_concrete_string(artifact.get("kind"), f"{context}.kind")
    if kind != _EXECUTION_CONTEXT_KIND:
        raise ValueError(f"{context}.kind must be {_EXECUTION_CONTEXT_KIND!r}")
    version = expect_concrete_string(
        artifact.get("schema_version"),
        f"{context}.schema_version",
    )
    if version != SCHEMA_VERSION:
        raise ValueError(f"{context}.schema_version must be {SCHEMA_VERSION!r}")

    body_context = f"{context}.context"
    body = expect_mapping(artifact.get("context"), body_context)
    expect_exact_keys(
        body,
        {
            "compiler",
            "runtime",
            "target",
            "backend",
            "device",
            "driver_runtime",
            "determinism",
            "parallelism",
            "active_preconditions",
        },
        body_context,
    )
    return context_model.ExecutionContext(
        compiler=_parse_revisioned_component(
            body.get("compiler"),
            f"{body_context}.compiler",
        ),
        runtime=_parse_revisioned_component(
            body.get("runtime"),
            f"{body_context}.runtime",
        ),
        target=_parse_target(body.get("target"), f"{body_context}.target"),
        backend=_parse_versioned_component(
            body.get("backend"),
            f"{body_context}.backend",
        ),
        device=_parse_device(body.get("device"), f"{body_context}.device"),
        driver_runtime=_parse_versioned_component(
            body.get("driver_runtime"),
            f"{body_context}.driver_runtime",
        ),
        determinism=_parse_determinism(
            body.get("determinism"),
            f"{body_context}.determinism",
        ),
        parallelism=_parse_parallelism(
            body.get("parallelism"),
            f"{body_context}.parallelism",
        ),
        active_preconditions=_parse_preconditions(
            body.get("active_preconditions"),
            f"{body_context}.active_preconditions",
        ),
    )


def _versioned_component_payload(
    component: context_model.VersionedComponent,
) -> dict[str, JsonValue]:
    """Return the canonical discriminated component payload."""
    if isinstance(component, context_model.NotApplicableComponent):
        return {"applicability": "not_applicable"}
    return {
        "applicability": "identified",
        "name": component.name,
        "version": component.version,
        "revision": component.revision,
    }


def _device_payload(device: context_model.Device) -> dict[str, JsonValue]:
    """Return the canonical discriminated device payload."""
    if isinstance(device, context_model.NotApplicableDevice):
        return {"applicability": "not_applicable"}
    return {
        "applicability": "identified",
        "kind": device.kind,
        "vendor": device.vendor,
        "model": device.model,
        "ordinal": device.ordinal,
    }


def _randomness_payload(randomness: context_model.Randomness) -> dict[str, JsonValue]:
    """Return the canonical discriminated randomness payload."""
    if isinstance(randomness, context_model.UnusedRandomness):
        return {"mode": "unused"}
    if isinstance(randomness, context_model.FixedSeedRandomness):
        return {
            "mode": "fixed_seed",
            "algorithm": randomness.algorithm,
            "seed": randomness.seed,
        }
    return {
        "mode": "system_entropy",
        "source": randomness.source,
    }


def execution_context_payload(
    context: context_model.ExecutionContext,
) -> dict[str, JsonValue]:
    """Return an execution context as canonical JSON data.

    Parameters
    ----------
    context : context_model.ExecutionContext
        Execution context to encode.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical artifact payload.
    """
    return {
        "kind": _EXECUTION_CONTEXT_KIND,
        "schema_version": SCHEMA_VERSION,
        "context": {
            "compiler": {
                "name": context.compiler.name,
                "version": context.compiler.version,
                "revision": context.compiler.revision,
            },
            "runtime": {
                "name": context.runtime.name,
                "version": context.runtime.version,
                "revision": context.runtime.revision,
            },
            "target": {
                "triple": context.target.triple,
            },
            "backend": _versioned_component_payload(context.backend),
            "device": _device_payload(context.device),
            "driver_runtime": _versioned_component_payload(context.driver_runtime),
            "determinism": {
                "default_profile": context.determinism.default_profile,
                "randomness": _randomness_payload(context.determinism.randomness),
            },
            "parallelism": {
                "workers": context.parallelism.workers,
                "threads": context.parallelism.threads,
                "processes": context.parallelism.processes,
            },
            "active_preconditions": list(context.active_preconditions),
        },
    }


def execution_context_digest(context: context_model.ExecutionContext) -> str:
    """Return the canonical content address for an execution context.

    Parameters
    ----------
    context : context_model.ExecutionContext
        Execution context to address.

    Returns
    -------
    str
        Lowercase SHA-256 content address with its algorithm prefix.
    """
    return canonical_digest(execution_context_payload(context))

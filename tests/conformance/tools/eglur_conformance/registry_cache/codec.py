"""Decode the strict registry-cache probe YAML contract."""

import re
from collections import Counter
from collections.abc import Set as AbstractSet
from typing import cast

from eglur_conformance.registry_cache import model
from eglur_conformance.suite import model as suite_model
from eglur_conformance.yaml_document import YamlValue

_ROOT_KEYS = frozenset(
    {"profile", "requires_capability", "fixture_manifest", "observations", "cases"}
)
_COMMON_CASE_KEYS = frozenset({"id", "title", "scenario"})
_TEST_IDENTITY_KEYS = frozenset({"suite_id", "test_id"})
_CASE_ID_PATTERN = re.compile(r"RCACHE-[a-z0-9]+(?:-[a-z0-9]+)*\Z")


def _mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def _list(value: YamlValue, context: str) -> list[YamlValue]:
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def _text(value: YamlValue, context: str) -> str:
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if not value:
        raise ValueError(f"{context} must be non-empty")
    return value


def _exact_keys(
    value: dict[str, YamlValue],
    expected: AbstractSet[str],
    context: str,
) -> None:
    missing = sorted(expected - set(value))
    unsupported = sorted(set(value) - expected)
    if missing or unsupported:
        details: list[str] = []
        if missing:
            details.append(f"missing {missing}")
        if unsupported:
            details.append(f"unsupported {unsupported}")
        raise ValueError(f"{context} has invalid keys: {', '.join(details)}")


def _decode_test_identity(
    value: YamlValue,
    context: str,
) -> suite_model.TestIdentity:
    reference = _mapping(value, context)
    _exact_keys(reference, _TEST_IDENTITY_KEYS, context)
    return suite_model.TestIdentity(
        suite_id=_text(reference.get("suite_id"), f"{context}.suite_id"),
        test_id=_text(reference.get("test_id"), f"{context}.test_id"),
    )


def _decode_case(value: YamlValue, context: str) -> model.ProbeCase:
    case = _mapping(value, context)
    case_id = _text(case.get("id"), f"{context}.id")
    if _CASE_ID_PATTERN.fullmatch(case_id) is None:
        raise ValueError(f"{context}.id must be a canonical RCACHE semantic ID")
    title = _text(case.get("title"), f"{context}.title")
    scenario = _text(case.get("scenario"), f"{context}.scenario")

    if scenario in model.SINGLE_FIXTURE_SCENARIOS:
        _exact_keys(case, _COMMON_CASE_KEYS | {"fixture"}, context)
        return model.SingleFixtureCase(
            case_id=case_id,
            title=title,
            scenario=cast(model.SingleFixtureScenario, scenario),
            fixture=_decode_test_identity(
                case.get("fixture"),
                f"{context}.fixture",
            ),
        )
    if scenario == "complete_coordinate_isolation":
        _exact_keys(
            case,
            _COMMON_CASE_KEYS
            | {
                "priming_fixture",
                "package_fixture",
                "version_fixture",
                "integrity_fixture",
            },
            context,
        )
        return model.CoordinateIsolationCase(
            case_id=case_id,
            title=title,
            priming_fixture=_decode_test_identity(
                case.get("priming_fixture"),
                f"{context}.priming_fixture",
            ),
            package_fixture=_decode_test_identity(
                case.get("package_fixture"),
                f"{context}.package_fixture",
            ),
            version_fixture=_decode_test_identity(
                case.get("version_fixture"),
                f"{context}.version_fixture",
            ),
            integrity_fixture=_decode_test_identity(
                case.get("integrity_fixture"),
                f"{context}.integrity_fixture",
            ),
        )
    if scenario == "concurrent_same_coordinate_converges":
        _exact_keys(
            case,
            _COMMON_CASE_KEYS
            | {
                "fixture",
                "participants",
                "publication_pause",
                "lookup_rendezvous",
            },
            context,
        )
        participants = case.get("participants")
        if type(participants) is not int:
            raise TypeError(f"{context}.participants must be an integer")
        if participants != 2:
            raise ValueError(
                f"{context}.participants must be 2 for {model.PROFILE_NAME}"
            )
        publication_pause = _text(
            case.get("publication_pause"),
            f"{context}.publication_pause",
        )
        if publication_pause != model.PUBLICATION_PAUSE:
            raise ValueError(
                f"{context}.publication_pause must be {model.PUBLICATION_PAUSE!r}"
            )
        lookup_rendezvous = _text(
            case.get("lookup_rendezvous"),
            f"{context}.lookup_rendezvous",
        )
        if lookup_rendezvous != model.LOOKUP_RENDEZVOUS:
            raise ValueError(
                f"{context}.lookup_rendezvous must be {model.LOOKUP_RENDEZVOUS!r}"
            )
        return model.ConcurrentConvergenceCase(
            case_id=case_id,
            title=title,
            fixture=_decode_test_identity(
                case.get("fixture"),
                f"{context}.fixture",
            ),
            participants=participants,
            publication_pause=model.PUBLICATION_PAUSE,
            lookup_rendezvous=model.LOOKUP_RENDEZVOUS,
        )
    raise ValueError(f"{context}.scenario is unsupported: {scenario}")


def decode_profile(
    document: YamlValue,
    context: str = "registry_cache_profile",
) -> model.RegistryCacheProfile:
    """Decode one closed `registry-cache-v1` profile.

    Parameters
    ----------
    document : YamlValue
        Parsed YAML value to validate and normalize.
    context : str
        Human-readable source label for validation failures.

    Returns
    -------
    RegistryCacheProfile
        Canonical profile model.

    Raises
    ------
    TypeError
        If a value has the wrong YAML kind.
    ValueError
        If the document violates the closed profile schema.
    """
    root = _mapping(document, context)
    _exact_keys(root, _ROOT_KEYS, context)

    profile_name = _text(root.get("profile"), f"{context}.profile")
    if profile_name != model.PROFILE_NAME:
        raise ValueError(
            f"{context}.profile must be {model.PROFILE_NAME!r}, found {profile_name!r}"
        )
    required_capability = _text(
        root.get("requires_capability"),
        f"{context}.requires_capability",
    )
    if required_capability != model.REQUIRED_CAPABILITY:
        raise ValueError(
            f"{context}.requires_capability must be {model.REQUIRED_CAPABILITY!r}"
        )
    fixture_manifest = _text(
        root.get("fixture_manifest"),
        f"{context}.fixture_manifest",
    )
    if fixture_manifest != model.FIXTURE_MANIFEST_PATH:
        raise ValueError(
            f"{context}.fixture_manifest must be {model.FIXTURE_MANIFEST_PATH!r}"
        )

    raw_observations = _list(
        root.get("observations"),
        f"{context}.observations",
    )
    observations = tuple(
        _text(item, f"{context}.observations[{index}]")
        for index, item in enumerate(raw_observations)
    )
    if len(observations) != len(set(observations)):
        raise ValueError(f"{context}.observations must not contain duplicates")
    if set(observations) != set(model.REGISTRY_CACHE_OBSERVATIONS):
        missing = sorted(set(model.REGISTRY_CACHE_OBSERVATIONS) - set(observations))
        unsupported = sorted(set(observations) - set(model.REGISTRY_CACHE_OBSERVATIONS))
        raise ValueError(
            f"{context}.observations must match the closed profile: "
            f"missing={missing}, unsupported={unsupported}"
        )

    raw_cases = _list(root.get("cases"), f"{context}.cases")
    if not raw_cases:
        raise ValueError(f"{context}.cases must not be empty")
    decoded_cases = tuple(
        _decode_case(item, f"{context}.cases[{index}]")
        for index, item in enumerate(raw_cases)
    )
    duplicate_ids = sorted(
        case_id
        for case_id, count in Counter(case.case_id for case in decoded_cases).items()
        if count > 1
    )
    if duplicate_ids:
        raise ValueError(f"{context}.cases has duplicate ids {duplicate_ids}")
    scenarios = {case.scenario for case in decoded_cases}
    missing_scenarios = sorted(set(model.REQUIRED_SCENARIOS) - scenarios)
    if missing_scenarios:
        raise ValueError(
            f"{context}.cases is missing required scenarios {missing_scenarios}"
        )
    duplicate_scenarios = sorted(
        scenario
        for scenario, count in Counter(case.scenario for case in decoded_cases).items()
        if count > 1
    )
    if duplicate_scenarios:
        raise ValueError(
            f"{context}.cases has duplicate scenarios {duplicate_scenarios}"
        )

    return model.RegistryCacheProfile(
        required_capability=model.REQUIRED_CAPABILITY,
        fixture_manifest=fixture_manifest,
        observations=model.REGISTRY_CACHE_OBSERVATIONS,
        cases=tuple(sorted(decoded_cases, key=lambda case: case.case_id)),
    )

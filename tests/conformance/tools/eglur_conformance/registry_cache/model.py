"""Canonical models for the registry-cache implementation probe."""

from dataclasses import dataclass
from typing import Literal, TypeAlias

from eglur_conformance.suite import model as suite_model

PROFILE_NAME = "registry-cache-v1"
FIXTURE_MANIFEST_PATH = "tests/conformance/manifest.yaml"
REQUIRED_CAPABILITY = "shared_registry_cache"
PUBLICATION_PAUSE = "before_logical_commit"
LOOKUP_RENDEZVOUS = "second_lookup_entered"

RegistryCacheCapability: TypeAlias = Literal["shared_registry_cache"]

RegistryCacheObservation: TypeAlias = Literal[
    "cache_lookup_result",
    "cache_publication_result",
    "provider_requests",
    "invocation_result",
    "committed_snapshot_visibility",
]
REGISTRY_CACHE_OBSERVATIONS: tuple[RegistryCacheObservation, ...] = (
    "cache_lookup_result",
    "cache_publication_result",
    "provider_requests",
    "invocation_result",
    "committed_snapshot_visibility",
)

SingleFixtureScenario: TypeAlias = Literal[
    "cold_then_warm",
    "failed_acquisition_not_published",
    "invalid_package_not_published",
    "cache_lookup_unavailable_fallback",
    "cache_lookup_unavailable_preserves_provider_failure",
    "cache_publication_unavailable_fallback",
    "recovered_incomplete_is_miss",
]
SINGLE_FIXTURE_SCENARIOS: tuple[SingleFixtureScenario, ...] = (
    "cold_then_warm",
    "failed_acquisition_not_published",
    "invalid_package_not_published",
    "cache_lookup_unavailable_fallback",
    "cache_lookup_unavailable_preserves_provider_failure",
    "cache_publication_unavailable_fallback",
    "recovered_incomplete_is_miss",
)

ProbeScenario: TypeAlias = Literal[
    "cold_then_warm",
    "complete_coordinate_isolation",
    "failed_acquisition_not_published",
    "invalid_package_not_published",
    "cache_lookup_unavailable_fallback",
    "cache_lookup_unavailable_preserves_provider_failure",
    "cache_publication_unavailable_fallback",
    "recovered_incomplete_is_miss",
    "concurrent_same_coordinate_converges",
]
REQUIRED_SCENARIOS: tuple[ProbeScenario, ...] = (
    "cold_then_warm",
    "complete_coordinate_isolation",
    "failed_acquisition_not_published",
    "invalid_package_not_published",
    "cache_lookup_unavailable_fallback",
    "cache_lookup_unavailable_preserves_provider_failure",
    "cache_publication_unavailable_fallback",
    "recovered_incomplete_is_miss",
    "concurrent_same_coordinate_converges",
)


@dataclass(frozen=True)
class SingleFixtureCase:
    """Define a cache scenario parameterized by one source fixture.

    Parameters
    ----------
    case_id : str
        Stable profile-local case identity.
    title : str
        Human-readable case title.
    scenario : SingleFixtureScenario
        Closed state-transition oracle selected by this case.
    fixture : TestIdentity
        Source invocation reused by the probe.
    """

    case_id: str
    title: str
    scenario: SingleFixtureScenario
    fixture: suite_model.TestIdentity

    def __post_init__(self) -> None:
        """Reject an invalid programmatic single-fixture case."""
        if type(self.case_id) is not str or not self.case_id:
            raise TypeError("registry-cache case_id must be a non-empty string")
        if type(self.title) is not str or not self.title:
            raise TypeError("registry-cache title must be a non-empty string")
        if self.scenario not in SINGLE_FIXTURE_SCENARIOS:
            raise ValueError(f"unsupported single-fixture scenario: {self.scenario}")
        if not isinstance(self.fixture, suite_model.TestIdentity):
            raise TypeError("single-fixture case requires a TestIdentity")


@dataclass(frozen=True)
class CoordinateIsolationCase:
    """Define one complete-coordinate isolation scenario.

    Parameters
    ----------
    case_id : str
        Stable profile-local case identity.
    title : str
        Human-readable case title.
    priming_fixture : TestIdentity
        Successful fixture whose verified snapshot primes the cache.
    package_fixture : TestIdentity
        Fixture that changes only the registry package ID.
    version_fixture : TestIdentity
        Fixture that changes only the exact version.
    integrity_fixture : TestIdentity
        Fixture that changes only the tree integrity.
    """

    case_id: str
    title: str
    priming_fixture: suite_model.TestIdentity
    package_fixture: suite_model.TestIdentity
    version_fixture: suite_model.TestIdentity
    integrity_fixture: suite_model.TestIdentity

    def __post_init__(self) -> None:
        """Reject an incomplete coordinate-isolation case identity."""
        if type(self.case_id) is not str or not self.case_id:
            raise TypeError("registry-cache case_id must be a non-empty string")
        if type(self.title) is not str or not self.title:
            raise TypeError("registry-cache title must be a non-empty string")
        references = (
            self.priming_fixture,
            self.package_fixture,
            self.version_fixture,
            self.integrity_fixture,
        )
        if not all(isinstance(item, suite_model.TestIdentity) for item in references):
            raise TypeError("coordinate-isolation fixtures must be TestIdentity values")

    @property
    def scenario(self) -> Literal["complete_coordinate_isolation"]:
        """Return the closed scenario identity.

        Returns
        -------
        Literal["complete_coordinate_isolation"]
            Scenario tag for codec and policy dispatch.
        """
        return "complete_coordinate_isolation"


@dataclass(frozen=True)
class ConcurrentConvergenceCase:
    """Define one same-coordinate concurrent-publication scenario.

    Parameters
    ----------
    case_id : str
        Stable profile-local case identity.
    title : str
        Human-readable case title.
    fixture : TestIdentity
        Successful source invocation run by both participants.
    participants : int
        Concurrent invocation count, fixed to two in `registry-cache-v1`.
    publication_pause : Literal["before_logical_commit"]
        Harness pause point that makes partial visibility observable.
    lookup_rendezvous : Literal["second_lookup_entered"]
        Harness rendezvous required before publication resumes.
    """

    case_id: str
    title: str
    fixture: suite_model.TestIdentity
    participants: int
    publication_pause: Literal["before_logical_commit"]
    lookup_rendezvous: Literal["second_lookup_entered"]

    def __post_init__(self) -> None:
        """Reject concurrency controls outside the closed profile."""
        if type(self.case_id) is not str or not self.case_id:
            raise TypeError("registry-cache case_id must be a non-empty string")
        if type(self.title) is not str or not self.title:
            raise TypeError("registry-cache title must be a non-empty string")
        if not isinstance(self.fixture, suite_model.TestIdentity):
            raise TypeError("concurrent case requires a TestIdentity")
        if type(self.participants) is not int:
            raise TypeError("participants must be an integer")
        if self.participants != 2:
            raise ValueError(f"participants must be 2 for {PROFILE_NAME}")
        if self.publication_pause != PUBLICATION_PAUSE:
            raise ValueError(
                f"publication_pause must be {PUBLICATION_PAUSE!r} for {PROFILE_NAME}"
            )
        if self.lookup_rendezvous != LOOKUP_RENDEZVOUS:
            raise ValueError(
                f"lookup_rendezvous must be {LOOKUP_RENDEZVOUS!r} for {PROFILE_NAME}"
            )

    @property
    def scenario(self) -> Literal["concurrent_same_coordinate_converges"]:
        """Return the closed scenario identity.

        Returns
        -------
        Literal["concurrent_same_coordinate_converges"]
            Scenario tag for codec and policy dispatch.
        """
        return "concurrent_same_coordinate_converges"


ProbeCase: TypeAlias = (
    SingleFixtureCase | CoordinateIsolationCase | ConcurrentConvergenceCase
)


@dataclass(frozen=True)
class RegistryCacheProfile:
    """Canonical `registry-cache-v1` profile definition.

    Parameters
    ----------
    required_capability : RegistryCacheCapability
        Capability claim that makes this implementation profile applicable.
    fixture_manifest : str
        Repository path of the source-fixture manifest.
    observations : tuple[RegistryCacheObservation, ...]
        Complete abstract observation vocabulary.
    cases : tuple[ProbeCase, ...]
        Profile cases with closed scenario semantics.
    """

    required_capability: RegistryCacheCapability
    fixture_manifest: str
    observations: tuple[RegistryCacheObservation, ...]
    cases: tuple[ProbeCase, ...]

    def __post_init__(self) -> None:
        """Reject a noncanonical programmatic profile."""
        if self.required_capability != REQUIRED_CAPABILITY:
            raise ValueError(f"required_capability must be {REQUIRED_CAPABILITY!r}")
        if self.fixture_manifest != FIXTURE_MANIFEST_PATH:
            raise ValueError(f"fixture_manifest must be {FIXTURE_MANIFEST_PATH!r}")
        if self.observations != REGISTRY_CACHE_OBSERVATIONS:
            raise ValueError("observations must match the canonical profile order")
        if type(self.cases) is not tuple or not all(
            isinstance(
                case,
                (SingleFixtureCase, CoordinateIsolationCase, ConcurrentConvergenceCase),
            )
            for case in self.cases
        ):
            raise TypeError("cases must be a tuple of registry-cache case models")
        case_ids = tuple(case.case_id for case in self.cases)
        if case_ids != tuple(sorted(case_ids)) or len(case_ids) != len(set(case_ids)):
            raise ValueError("cases must have unique IDs in canonical lexical order")
        scenarios = tuple(case.scenario for case in self.cases)
        if len(scenarios) != len(REQUIRED_SCENARIOS) or set(scenarios) != set(
            REQUIRED_SCENARIOS
        ):
            raise ValueError("cases must contain each required scenario exactly once")

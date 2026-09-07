"""Canonical models for the first semantic fixture projection."""

from dataclasses import dataclass

from eglur_conformance.artifact_content import DIGEST_PATTERN, SCHEMA_VERSION
from eglur_conformance.suite import model as suite_model

CORE_V1_FIXTURE_COUNT = 23


@dataclass(frozen=True)
class FixtureSelection:
    """Authored manifest-qualified fixture identities.

    Parameters
    ----------
    schema_version : str
        Selection schema version.
    identities : tuple[suite_model.TestIdentity, ...]
        Sorted, unique fixture identities without copied expectations.

    Raises
    ------
    TypeError
        ``identities`` is not a tuple of canonical test identities.
    ValueError
        The schema is unsupported or the selection is empty, unordered, or
        contains duplicate identities.
    """

    schema_version: str
    identities: tuple[suite_model.TestIdentity, ...]

    def __post_init__(self) -> None:
        """Reject malformed, empty, duplicate, or unordered selections."""
        if self.schema_version != SCHEMA_VERSION:
            raise ValueError(
                f"fixture selection schema_version must be {SCHEMA_VERSION!r}"
            )
        if type(self.identities) is not tuple or not all(
            isinstance(identity, suite_model.TestIdentity)
            for identity in self.identities
        ):
            raise TypeError(
                "fixture selection identities must be a tuple of TestIdentity values"
            )
        if not self.identities:
            raise ValueError("fixture selection identities must not be empty")
        if self.identities != tuple(sorted(self.identities)):
            raise ValueError("fixture selection identities must be sorted")
        if len(set(self.identities)) != len(self.identities):
            raise ValueError("fixture selection identities must be unique")


@dataclass(frozen=True)
class SemanticFixture:
    """One selected compiler invocation and its language expectation.

    Parameters
    ----------
    identity : suite_model.TestIdentity
        Manifest-qualified canonical fixture identity.
    invocation : suite_model.FixtureInvocation
        Exact source and runner invocation from the conformance bundle.
    expectation : suite_model.Expectation
        Complete selected language outcome and secondary assertions.

    Raises
    ------
    TypeError
        A canonical input has the wrong model type or outcome family.
    ValueError
        The invocation or expectation uses an unsupported first-slice form.
    """

    identity: suite_model.TestIdentity
    invocation: suite_model.FixtureInvocation
    expectation: suite_model.Expectation

    def __post_init__(self) -> None:
        """Enforce the bounded first-slice fixture surface."""
        if not isinstance(self.identity, suite_model.TestIdentity):
            raise TypeError("semantic fixture identity must be a TestIdentity")
        if not isinstance(self.invocation, suite_model.FixtureInvocation):
            raise TypeError("semantic fixture invocation must be a FixtureInvocation")
        if not isinstance(self.expectation, suite_model.Expectation):
            raise TypeError("semantic fixture expectation must be an Expectation")
        if self.invocation.registry_acquisitions:
            raise ValueError(
                "semantic fixture corpus does not support registry acquisitions"
            )
        outcome = self.expectation.outcome
        if not isinstance(
            outcome,
            (suite_model.ValueOutcome, suite_model.CompileErrorOutcome),
        ):
            raise TypeError(
                "semantic fixture corpus supports only value and compile-error outcomes"
            )
        if (
            isinstance(outcome, suite_model.ValueOutcome)
            and outcome.numeric_comparison is not None
        ):
            raise ValueError(
                "semantic fixture corpus does not support numeric comparison outcomes"
            )


@dataclass(frozen=True)
class SemanticFixtureCorpus:
    """Deterministic generated corpus for the first semantic compiler slice.

    Parameters
    ----------
    schema_version : str
        Generated artifact schema version.
    input_digest : str
        Content address of the exact selection and conformance bundle.
    fixtures : tuple[SemanticFixture, ...]
        Sorted, unique selected fixtures.

    Raises
    ------
    TypeError
        ``fixtures`` is not a tuple of semantic fixtures.
    ValueError
        Provenance is malformed or the fixture set is not the exact core-v1
        corpus.
    """

    schema_version: str
    input_digest: str
    fixtures: tuple[SemanticFixture, ...]

    def __post_init__(self) -> None:
        """Reject malformed provenance and ambiguous fixture identity."""
        if self.schema_version != SCHEMA_VERSION:
            raise ValueError(
                f"semantic corpus schema_version must be {SCHEMA_VERSION!r}"
            )
        if (
            type(self.input_digest) is not str
            or DIGEST_PATTERN.fullmatch(self.input_digest) is None
        ):
            raise ValueError("semantic corpus input_digest must be a SHA-256 address")
        if type(self.fixtures) is not tuple or not all(
            isinstance(fixture, SemanticFixture) for fixture in self.fixtures
        ):
            raise TypeError(
                "semantic corpus fixtures must be a tuple of SemanticFixture values"
            )
        if len(self.fixtures) != CORE_V1_FIXTURE_COUNT:
            raise ValueError(
                "core-v1 semantic corpus must contain exactly "
                f"{CORE_V1_FIXTURE_COUNT} fixtures, found {len(self.fixtures)}"
            )
        identities = tuple(fixture.identity for fixture in self.fixtures)
        if identities != tuple(sorted(identities)):
            raise ValueError("semantic corpus fixture identities must be sorted")
        if len(set(identities)) != len(identities):
            raise ValueError("semantic corpus fixture identities must be unique")

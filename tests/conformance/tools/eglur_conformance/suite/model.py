"""Canonical conformance suite models."""

from dataclasses import dataclass
from typing import Literal, TypeAlias

from eglur_conformance.diagnostics.model import ByteRange, DiagnosticScalar

OPTIONAL_PROBE_CATEGORY = "optional_emit_if_present"


@dataclass(frozen=True)
class BaselineParticipation:
    """An unconditionally required baseline suite."""


@dataclass(frozen=True)
class ConditionalParticipation:
    """A required suite activated by one recognized precondition."""

    precondition: str


@dataclass(frozen=True)
class OptionalParticipation:
    """A non-baseline suite that never blocks the v0.1 gate."""


SuiteParticipation: TypeAlias = (
    BaselineParticipation | ConditionalParticipation | OptionalParticipation
)


@dataclass(frozen=True, order=True)
class TestIdentity:
    """Identify one manifest-owned conformance test.

    Parameters
    ----------
    suite_id : str
        Exact suite identity from the conformance manifest.
    test_id : str
        Exact test identity from the referenced suite.

    Raises
    ------
    TypeError
        If either component is not a string.
    ValueError
        If either component is empty.
    """

    suite_id: str
    test_id: str

    def __post_init__(self) -> None:
        """Reject incomplete or mistyped identity components."""
        if type(self.suite_id) is not str:
            raise TypeError("suite_id must be a string")
        if not self.suite_id:
            raise ValueError("suite_id must be non-empty")
        if type(self.test_id) is not str:
            raise TypeError("test_id must be a string")
        if not self.test_id:
            raise ValueError("test_id must be non-empty")

    def __str__(self) -> str:
        """Return the established suite/test diagnostic label.

        Returns
        -------
        str
            The identity rendered as ``suite_id/test_id``.
        """
        return f"{self.suite_id}/{self.test_id}"


@dataclass(frozen=True)
class SuiteEntry:
    """One manifest suite entry with a closed participation state."""

    suite_id: str
    file: str
    participation: SuiteParticipation

    @property
    def required(self) -> bool:
        """Return whether this suite participates in the baseline gate."""
        return not isinstance(self.participation, OptionalParticipation)

    @property
    def precondition(self) -> str | None:
        """Return the activation precondition for a conditional suite."""
        if isinstance(self.participation, ConditionalParticipation):
            return self.participation.precondition
        return None


@dataclass(frozen=True)
class Manifest:
    """Canonical conformance manifest."""

    version: str
    release_history_root: str
    release: str
    status: str
    root: str
    suites: tuple[SuiteEntry, ...]


FixtureMode: TypeAlias = Literal["auto", "package", "file-set"]
FixtureRunner: TypeAlias = Literal["main", "test"]


@dataclass(frozen=True)
class Utf8Source:
    """One inline UTF-8 Eglur source."""

    text: str


@dataclass(frozen=True)
class RawBytesSource:
    """One inline byte source represented by canonical lowercase hex."""

    hex_bytes: str


@dataclass(frozen=True)
class SourceFile:
    """One path and text entry in a multi-file fixture."""

    path: str
    text: str


@dataclass(frozen=True)
class FixtureSymlink:
    """One symbolic-link declaration in a file-set fixture.

    Parameters
    ----------
    path : str
        Canonical relative POSIX path of the link node.
    target : str
        Relative POSIX target interpreted from the link's parent directory.
    """

    path: str
    target: str


@dataclass(frozen=True)
class FileSetSource:
    """A file-set or package fixture with one designated entry source.

    Parameters
    ----------
    files : tuple[SourceFile, ...]
        Canonically ordered regular files.
    symlinks : tuple[FixtureSymlink, ...]
        Canonically ordered symbolic-link topology.
    entry : str
        Mapped Eglur source selected as the compile entry.
    root : str or None
        Explicit logical file-set root, when required by the invocation mode.
    """

    files: tuple[SourceFile, ...]
    symlinks: tuple[FixtureSymlink, ...]
    entry: str
    root: str | None


FixtureSource: TypeAlias = Utf8Source | RawBytesSource | FileSetSource


RegistryLinkKind: TypeAlias = Literal["symlink", "hardlink"]
RegistrySpecialKind: TypeAlias = Literal[
    "block_device",
    "character_device",
    "fifo",
    "socket",
    "other_special",
]
RegistryProviderFinish: TypeAlias = Literal[
    "complete",
    "missing_release",
    "transport_failure",
]
REGISTRY_PROVIDER_FINISHES: tuple[RegistryProviderFinish, ...] = (
    "complete",
    "missing_release",
    "transport_failure",
)
REGISTRY_PROVIDER_REPEAT_COUNT_MAX = (1 << 63) - 1


@dataclass(frozen=True)
class RegistryRepeatedByte:
    """One byte repeated without materializing the represented sequence.

    Parameters
    ----------
    byte : int
        Byte value in the inclusive range 0 through 255.
    count : int
        Positive repetition count representable as `i64`.
    """

    byte: int
    count: int

    def __post_init__(self) -> None:
        """Reject values outside the compact provider-fixture domain."""
        if type(self.byte) is not int or not 0 <= self.byte <= 255:
            raise ValueError("registry repeated byte must be in range 0 through 255")
        if (
            type(self.count) is not int
            or not 0 < self.count <= REGISTRY_PROVIDER_REPEAT_COUNT_MAX
        ):
            raise ValueError("registry repeated byte count must be a positive i64")


RegistryBytes: TypeAlias = bytes | RegistryRepeatedByte


@dataclass(frozen=True)
class RegistryFileEntry:
    """One exact regular-file entry emitted by a registry provider.

    Parameters
    ----------
    path : RegistryBytes
        Raw package-root-relative path byte source.
    content : RegistryBytes
        Exact file-content byte source.
    """

    path: RegistryBytes
    content: RegistryBytes


@dataclass(frozen=True)
class RegistryDirectoryEntry:
    """One explicit directory marker emitted by a registry provider.

    Parameters
    ----------
    path : RegistryBytes
        Raw package-root-relative path byte source.
    """

    path: RegistryBytes


@dataclass(frozen=True)
class RegistryLinkEntry:
    """One symbolic- or hard-link entry emitted by a registry provider.

    Parameters
    ----------
    kind : RegistryLinkKind
        Closed link kind.
    path : RegistryBytes
        Raw package-root-relative link path byte source.
    target : RegistryBytes
        Exact raw target byte source.
    """

    kind: RegistryLinkKind
    path: RegistryBytes
    target: RegistryBytes


@dataclass(frozen=True)
class RegistrySpecialEntry:
    """One unsupported special entry emitted by a registry provider.

    Parameters
    ----------
    kind : RegistrySpecialKind
        Closed special-entry kind.
    path : RegistryBytes
        Raw package-root-relative path byte source.
    """

    kind: RegistrySpecialKind
    path: RegistryBytes


RegistryEntry: TypeAlias = (
    RegistryFileEntry
    | RegistryDirectoryEntry
    | RegistryLinkEntry
    | RegistrySpecialEntry
)


@dataclass(frozen=True)
class RegistryEmitStep:
    """Emit one logical entry into the compiler-owned acquisition sink.

    Parameters
    ----------
    entry : RegistryEntry
        Logical entry supplied by this sink call.
    """

    entry: RegistryEntry


@dataclass(frozen=True)
class RegistryRepeatStep:
    """Emit one logical entry repeatedly through the same sink boundary.

    Parameters
    ----------
    count : int
        Positive number of sequential sink emissions.
    entry : RegistryEntry
        Logical entry emitted on every repetition.
    """

    count: int
    entry: RegistryEntry

    def __post_init__(self) -> None:
        """Reject counts outside the positive `i64` fixture domain."""
        if (
            type(self.count) is not int
            or not 0 < self.count <= REGISTRY_PROVIDER_REPEAT_COUNT_MAX
        ):
            raise ValueError("registry provider repeat count must be a positive i64")


@dataclass(frozen=True)
class RegistryFinishStep:
    """Finish one provider attempt with a closed provider-owned outcome.

    Parameters
    ----------
    outcome : RegistryProviderFinish
        Provider signal emitted after all preceding entry operations.
    """

    outcome: RegistryProviderFinish

    def __post_init__(self) -> None:
        """Reject a finish value outside the closed provider-owned set."""
        if self.outcome not in REGISTRY_PROVIDER_FINISHES:
            raise ValueError(f"unsupported registry provider finish: {self.outcome}")


RegistryProviderStep: TypeAlias = (
    RegistryEmitStep | RegistryRepeatStep | RegistryFinishStep
)


@dataclass(frozen=True)
class RegistryAcquisition:
    """One exact registry provider attempt supplied to a compiler fixture.

    Parameters
    ----------
    package : str
        Canonical `RegistryPackageId`.
    version : str
        Canonical exact package version.
    provider_steps : tuple[RegistryProviderStep, ...]
        Ordered provider operations replayed through a fresh private sink.
    """

    package: str
    version: str
    provider_steps: tuple[RegistryProviderStep, ...]

    def __post_init__(self) -> None:
        """Reject provider scripts that cannot satisfy the terminal protocol."""
        if type(self.provider_steps) is not tuple or not self.provider_steps:
            raise ValueError("registry provider_steps must be a non-empty tuple")
        step_types = (RegistryEmitStep, RegistryRepeatStep, RegistryFinishStep)
        if not all(isinstance(step, step_types) for step in self.provider_steps):
            raise TypeError("registry provider_steps contains an unsupported step")
        if not isinstance(self.provider_steps[-1], RegistryFinishStep):
            raise TypeError("registry provider_steps must end with a finish step")
        if any(
            isinstance(step, RegistryFinishStep) for step in self.provider_steps[:-1]
        ):
            raise ValueError(
                "registry provider_steps cannot finish before the final step"
            )
        finish = self.provider_steps[-1]
        if finish.outcome == "missing_release" and len(self.provider_steps) != 1:
            raise ValueError("missing registry release cannot follow entry emissions")


@dataclass(frozen=True)
class FixtureInvocation:
    """Source and runner settings that determine compiler input.

    Parameters
    ----------
    source : FixtureSource
        Root source form selected by the fixture.
    registry_acquisitions : tuple[RegistryAcquisition, ...]
        Ordered registry provider attempts supplied to the compiler.
    mode : FixtureMode
        Requested package or file-set interpretation.
    runner : FixtureRunner
        Entry execution profile selected after compilation.
    """

    source: FixtureSource
    registry_acquisitions: tuple[RegistryAcquisition, ...]
    mode: FixtureMode
    runner: FixtureRunner


@dataclass(frozen=True)
class MarkerPrimarySpan:
    """A source marker resolved to one source unit and logical line.

    Parameters
    ----------
    marker : str
        Unique fixture-local marker name.
    source : str
        Canonical fixture source-unit identity.
    line : int
        One-based logical line containing the marker.
    """

    marker: str
    source: str
    line: int


@dataclass(frozen=True)
class BytePrimarySpan:
    """An exact physical byte-range assertion for one fixture source.

    Parameters
    ----------
    source : str
        Canonical fixture source-unit identity.
    byte_range : ByteRange
        Zero-based half-open physical source-byte range.
    """

    source: str
    byte_range: ByteRange


PrimarySpanAssertion: TypeAlias = MarkerPrimarySpan | BytePrimarySpan


@dataclass(frozen=True)
class DiagnosticAssertion:
    """One structured diagnostic expectation."""

    code: str
    message_all_of: tuple[str, ...]
    message_any_of: tuple[tuple[str, ...], ...]
    data_keys: tuple[str, ...]
    data_equals: tuple[tuple[str, DiagnosticScalar], ...]
    help_contains: tuple[str, ...] | None
    related_contains: tuple[str, ...] | None
    primary_span: PrimarySpanAssertion | None


NumericScalar: TypeAlias = str | int | float | bool
NumericExpected: TypeAlias = NumericScalar | tuple[NumericScalar, ...]
NumericComparisonMode: TypeAlias = Literal["exact", "approx"]


@dataclass(frozen=True)
class NumericComparison:
    """One scalar or flat tensor numeric comparison."""

    dtype: str
    expected: NumericExpected
    shape: tuple[int, ...] | None
    mode: NumericComparisonMode | None
    relative_tolerance: int | float | None
    absolute_tolerance: int | float | None


@dataclass(frozen=True)
class ValueOutcome:
    """One canonical value expectation."""

    kind: str
    representation: str | None
    error_variant: str | None
    numeric_comparison: NumericComparison | None


@dataclass(frozen=True)
class CompileErrorOutcome:
    """A compile rejection with one primary diagnostic."""

    diagnostic: DiagnosticAssertion


@dataclass(frozen=True)
class RuntimeErrorOutcome:
    """A runtime failure with one primary diagnostic."""

    diagnostic: DiagnosticAssertion


@dataclass(frozen=True)
class TestReference:
    """Identity of one test-runner result."""

    module: str
    name: str


TestFailureKind: TypeAlias = Literal["returned_err", "runtime_abort"]


@dataclass(frozen=True)
class TestFailure:
    """One failed test-runner result."""

    reference: TestReference
    kind: TestFailureKind
    message_contains: tuple[str, ...]
    runtime_code: str | None


@dataclass(frozen=True)
class TestRunOutcome:
    """Ordered test-runner results partitioned by terminal state."""

    order: tuple[TestReference, ...]
    passed: tuple[TestReference, ...]
    skipped: tuple[TestReference, ...]
    failed: tuple[TestFailure, ...]


ExpectedOutcome: TypeAlias = (
    CompileErrorOutcome | RuntimeErrorOutcome | ValueOutcome | TestRunOutcome
)


@dataclass(frozen=True)
class Expectation:
    """One primary outcome plus compatible secondary assertions."""

    outcome: ExpectedOutcome
    value_diagnostics: tuple[DiagnosticAssertion, ...]
    diagnostic_contains: tuple[str, ...] | None
    side_effects: tuple[str, ...]
    process_exit: int | None


@dataclass(frozen=True)
class TestCase:
    """One fully decoded conformance test case."""

    test_id: str
    category: str
    title: str
    invocation: FixtureInvocation
    expectation: Expectation


@dataclass(frozen=True)
class SuiteDocument:
    """One manifest-owned suite document."""

    tests: tuple[TestCase, ...]


@dataclass(frozen=True)
class ConformanceSuite:
    """A manifest entry paired with its exact decoded suite document."""

    entry: SuiteEntry
    document: SuiteDocument


@dataclass(frozen=True)
class ConformanceTest:
    """Bind one suite-local test case to its manifest suite.

    Parameters
    ----------
    suite : SuiteEntry
        Manifest entry that supplies the test's suite identity and participation.
    case : TestCase
        Decoded suite-local test case.

    Raises
    ------
    TypeError
        If either argument is not the declared suite model type.
    ValueError
        If the bound suite or test identity is empty.
    """

    suite: SuiteEntry
    case: TestCase

    def __post_init__(self) -> None:
        """Reject values outside the bound-test model."""
        if not isinstance(self.suite, SuiteEntry):
            raise TypeError("conformance test suite must be a SuiteEntry")
        if not isinstance(self.case, TestCase):
            raise TypeError("conformance test case must be a TestCase")
        _ = TestIdentity(self.suite.suite_id, self.case.test_id)

    @property
    def identity(self) -> TestIdentity:
        """Return the manifest-qualified test identity.

        Returns
        -------
        TestIdentity
            Identity derived from the bound suite and test case.
        """
        return TestIdentity(self.suite.suite_id, self.case.test_id)


@dataclass(frozen=True)
class ConformanceBundle:
    """One canonical manifest and its exact suite-document set."""

    manifest: Manifest
    suites: tuple[ConformanceSuite, ...]

    @property
    def tests(self) -> tuple[ConformanceTest, ...]:
        """Return tests bound to their manifest suite in source order.

        Returns
        -------
        tuple[ConformanceTest, ...]
            Every suite-local case paired with its suite entry.
        """
        return tuple(
            ConformanceTest(suite.entry, case)
            for suite in self.suites
            for case in suite.document.tests
        )

    def find_suite(self, suite_id: str) -> ConformanceSuite | None:
        """Find one suite by its manifest identity.

        Parameters
        ----------
        suite_id : str
            Exact suite identity to find.

        Returns
        -------
        ConformanceSuite | None
            Matching suite, or ``None`` when the identity is unknown.

        Raises
        ------
        TypeError
            If ``suite_id`` is not a string.
        """
        if type(suite_id) is not str:
            raise TypeError("suite lookup requires a string suite_id")
        return next(
            (suite for suite in self.suites if suite.entry.suite_id == suite_id),
            None,
        )

    def find_test(self, identity: TestIdentity) -> ConformanceTest | None:
        """Find one manifest-qualified test.

        Parameters
        ----------
        identity : TestIdentity
            Exact suite/test identity to find.

        Returns
        -------
        ConformanceTest | None
            Matching bound test, or ``None`` when the identity is unknown.

        Raises
        ------
        TypeError
            If ``identity`` is not a ``TestIdentity``.
        """
        if not isinstance(identity, TestIdentity):
            raise TypeError("test lookup requires a TestIdentity")
        return next((test for test in self.tests if test.identity == identity), None)

"""Canonical compilation evidence models."""

from dataclasses import dataclass
from typing import Literal, TypeAlias

from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.suite import model as suite_model

ScalarPayload: TypeAlias = str | int | bool


@dataclass(frozen=True)
class DiagnosticScalar:
    """One explicitly typed diagnostic-data scalar.

    Parameters
    ----------
    kind : str
        Closed scalar-kind name.
    value : ScalarPayload
        Scalar value matching ``kind``.
    """

    kind: str
    value: ScalarPayload


@dataclass(frozen=True)
class ObservedDiagnostic:
    """The primary structured error projected from compiler output.

    Parameters
    ----------
    severity : str
        Diagnostic severity.
    code : str
        Stable diagnostic code.
    message : str
        Human-readable diagnostic message.
    data : tuple[tuple[str, DiagnosticScalar], ...]
        Sorted structured diagnostic fields.
    primary_span : diagnostic_model.PrimarySpan
        Canonical source span or an explicit unavailable variant.
    """

    severity: str
    code: str
    message: str
    data: tuple[tuple[str, DiagnosticScalar], ...]
    primary_span: diagnostic_model.PrimarySpan


@dataclass(frozen=True)
class CompileSucceeded:
    """A compile-phase success without a whole-test verdict."""

    @property
    def kind(self) -> Literal["succeeded"]:
        """Return the serialized outcome kind.

        Returns
        -------
        Literal["succeeded"]
            The successful compile discriminator.
        """
        return "succeeded"


@dataclass(frozen=True)
class CompileRejected:
    """A compile-phase rejection with one primary diagnostic.

    Parameters
    ----------
    primary_diagnostic : ObservedDiagnostic
        Primary diagnostic selected by normative ordering.

    Raises
    ------
    TypeError
        ``primary_diagnostic`` is not an ``ObservedDiagnostic``.
    """

    primary_diagnostic: ObservedDiagnostic

    def __post_init__(self) -> None:
        if not isinstance(self.primary_diagnostic, ObservedDiagnostic):
            raise TypeError("primary_diagnostic must be an ObservedDiagnostic")

    @property
    def kind(self) -> Literal["rejected"]:
        """Return the serialized outcome kind.

        Returns
        -------
        Literal["rejected"]
            The rejected compile discriminator.
        """
        return "rejected"


CompileOutcome: TypeAlias = CompileSucceeded | CompileRejected


@dataclass(frozen=True)
class CompilerResult:
    """One compiler-native, single-document machine result.

    Parameters
    ----------
    outcome : CompileOutcome
        Structured compile outcome.
    """

    outcome: CompileOutcome


@dataclass(frozen=True)
class ExitedProcess:
    """A compiler process that exited with a status code.

    Parameters
    ----------
    code : int
        Non-negative process exit code.
    """

    code: int

    @property
    def kind(self) -> Literal["exited"]:
        """Return the serialized termination kind.

        Returns
        -------
        Literal["exited"]
            The normal-exit discriminator.
        """
        return "exited"


@dataclass(frozen=True)
class SignaledProcess:
    """A compiler process terminated by an operating-system signal.

    Parameters
    ----------
    signal : int
        Positive operating-system signal number.
    """

    signal: int

    @property
    def kind(self) -> Literal["signaled"]:
        """Return the serialized termination kind.

        Returns
        -------
        Literal["signaled"]
            The signal-termination discriminator.
        """
        return "signaled"


ProcessTermination: TypeAlias = ExitedProcess | SignaledProcess


@dataclass(frozen=True)
class CompileObservation:
    """One content-addressed compiler observation for one fixture.

    Parameters
    ----------
    suite_id : str
        Owning suite identifier.
    test_id : str
        Test identifier within the suite.
    specification_commit : str
        Full specification Git commit ID.
    run_id : str
        Declared compiler-run identifier.
    context_digest : str
        Execution-context content address.
    input_digest : str
        Compile-input content address.
    compiler_result_digest : str
        Structured compiler-result content address.
    log_digest : str
        Raw compiler-log content address.
    termination : ProcessTermination
        Compiler process termination.
    outcome : CompileOutcome
        Structured compile outcome.
    """

    suite_id: str
    test_id: str
    specification_commit: str
    run_id: str
    context_digest: str
    input_digest: str
    compiler_result_digest: str
    log_digest: str
    termination: ProcessTermination
    outcome: CompileOutcome

    @property
    def identity(self) -> suite_model.TestIdentity:
        """Return the normalized conformance-test identity.

        Returns
        -------
        suite_model.TestIdentity
            Identity formed from the serialized suite and test fields.

        Raises
        ------
        TypeError
            If either serialized identity component is not a string.
        ValueError
            If either serialized identity component is empty.
        """
        return suite_model.TestIdentity(self.suite_id, self.test_id)


@dataclass(frozen=True)
class SpecificationBinding:
    """One released specification identity and sign-off artifact address.

    Parameters
    ----------
    release : str
        Released specification label.
    commit : str
        Full specification Git commit ID.
    spec_package_signoff_digest : str
        Spec-package sign-off content address.
    conformance_bundle_digest : str
        Exact conformance-bundle content address.
    """

    release: str
    commit: str
    spec_package_signoff_digest: str
    conformance_bundle_digest: str


@dataclass(frozen=True)
class CompileEvidenceBinding:
    """The exact specification, run, context, and compile-observation set.

    Parameters
    ----------
    specification : SpecificationBinding
        Released specification and sign-off binding.
    run_id : str
        Declared compiler-run identifier.
    context_digest : str
        Selected execution-context content address.
    observation_digests : tuple[str, ...]
        Sorted compile-observation content addresses.
    """

    specification: SpecificationBinding
    run_id: str
    context_digest: str
    observation_digests: tuple[str, ...]

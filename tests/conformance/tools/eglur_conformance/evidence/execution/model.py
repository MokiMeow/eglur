"""Canonical execution evidence models."""

from dataclasses import dataclass
from typing import Literal, TypeAlias

from eglur_conformance.suite import model as suite_model


@dataclass(frozen=True)
class ResultRecord:
    """One test-level result identity and immutable result artifact binding.

    Parameters
    ----------
    suite_id : str
        Owning suite identifier.
    test_id : str
        Test identifier within the suite.
    run_id : str
        Declared execution-run identifier.
    context_digest : str
        Execution-context content address.
    result_digest : str
        Test-result content address.
    """

    suite_id: str
    test_id: str
    run_id: str
    context_digest: str
    result_digest: str

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
class ResultShard:
    """One content-addressed collection of explicit test-level result records.

    Parameters
    ----------
    records : tuple[ResultRecord, ...]
        Ordered test-level result records.
    """

    records: tuple[ResultRecord, ...]


@dataclass(frozen=True)
class ObservedTestResult:
    """One executed test result and its raw observation binding.

    Parameters
    ----------
    suite_id : str
        Owning suite identifier.
    test_id : str
        Test identifier within the suite.
    run_id : str
        Declared execution-run identifier.
    context_digest : str
        Execution-context content address.
    status : Literal["failed", "passed", "unsupported"]
        Executed terminal state.
    observed_result_digest : str
        Raw observed-result content address.
    log_digest : str
        Raw execution-log content address.
    """

    suite_id: str
    test_id: str
    run_id: str
    context_digest: str
    status: Literal["failed", "passed", "unsupported"]
    observed_result_digest: str
    log_digest: str

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
class NotRunTestResult:
    """One explicitly skipped test and its execution decision binding.

    Parameters
    ----------
    suite_id : str
        Owning suite identifier.
    test_id : str
        Test identifier within the suite.
    run_id : str
        Declared execution-run identifier.
    context_digest : str
        Execution-context content address.
    decision_digest : str
        Immutable non-execution decision content address.
    log_digest : str
        Raw decision-log content address.
    """

    suite_id: str
    test_id: str
    run_id: str
    context_digest: str
    decision_digest: str
    log_digest: str

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

    @property
    def status(self) -> Literal["not_run"]:
        """Return the serialized terminal state.

        Returns
        -------
        Literal["not_run"]
            The non-execution discriminator.
        """
        return "not_run"


TestResult: TypeAlias = ObservedTestResult | NotRunTestResult


@dataclass(frozen=True)
class EvidenceBinding:
    """The exact run, context, and result shards selected by one sign-off.

    Parameters
    ----------
    run_id : str
        Declared execution-run identifier.
    context_digest : str
        Selected execution-context content address.
    result_shard_digests : tuple[str, ...]
        Sorted result-shard content addresses.
    """

    run_id: str
    context_digest: str
    result_shard_digests: tuple[str, ...]

"""Canonical validation failures for conformance tooling."""

from dataclasses import dataclass


@dataclass(frozen=True)
class ValidationProblem:
    """One deterministic conformance validation failure.

    Parameters
    ----------
    message : str
        Failure description rendered by validation CLIs.
    """

    message: str

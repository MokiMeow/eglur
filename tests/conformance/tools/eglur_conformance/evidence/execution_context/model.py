"""Canonical execution-context evidence models."""

from dataclasses import dataclass
from typing import TypeAlias


@dataclass(frozen=True)
class RevisionedComponent:
    """One named compiler or runtime build.

    Parameters
    ----------
    name : str
        Component name.
    version : str
        Component version.
    revision : str
        Full Git object ID or SHA-256 content address.
    """

    name: str
    version: str
    revision: str


@dataclass(frozen=True)
class Target:
    """One fully identified compilation target.

    Parameters
    ----------
    triple : str
        Canonical target triple.
    """

    triple: str


@dataclass(frozen=True)
class NotApplicableComponent:
    """A backend or driver/runtime component absent from this context."""


VersionedComponent: TypeAlias = RevisionedComponent | NotApplicableComponent


@dataclass(frozen=True)
class IdentifiedDevice:
    """One fully identified execution device.

    Parameters
    ----------
    kind : str
        Device kind.
    vendor : str
        Device vendor.
    model : str
        Device model.
    ordinal : int
        Non-negative device ordinal.
    """

    kind: str
    vendor: str
    model: str
    ordinal: int


@dataclass(frozen=True)
class NotApplicableDevice:
    """An execution device absent from this context."""


Device: TypeAlias = IdentifiedDevice | NotApplicableDevice


@dataclass(frozen=True)
class UnusedRandomness:
    """A context in which no random source is used."""


@dataclass(frozen=True)
class FixedSeedRandomness:
    """A deterministic random source with an explicit algorithm and seed.

    Parameters
    ----------
    algorithm : str
        Random-number algorithm name.
    seed : str
        Seed represented without numeric-width loss.
    """

    algorithm: str
    seed: str


@dataclass(frozen=True)
class SystemEntropyRandomness:
    """A nondeterministic random source identified by its entropy provider.

    Parameters
    ----------
    source : str
        Entropy-provider identity.
    """

    source: str


Randomness: TypeAlias = UnusedRandomness | FixedSeedRandomness | SystemEntropyRandomness


@dataclass(frozen=True)
class Determinism:
    """The default tensor profile and randomness configuration.

    Parameters
    ----------
    default_profile : str
        Closed determinism-profile name.
    randomness : Randomness
        Randomness configuration for the run.
    """

    default_profile: str
    randomness: Randomness


@dataclass(frozen=True)
class Parallelism:
    """Resolved worker, thread, and process counts for one run.

    Parameters
    ----------
    workers : int
        Positive worker count.
    threads : int
        Positive thread count.
    processes : int
        Positive process count.
    """

    workers: int
    threads: int
    processes: int


@dataclass(frozen=True)
class ExecutionContext:
    """One complete execution context addressed by canonical content.

    Parameters
    ----------
    compiler : RevisionedComponent
        Compiler build identity.
    runtime : RevisionedComponent
        Runtime build identity.
    target : Target
        Compilation target.
    backend : VersionedComponent
        Backend identity or explicit inapplicability marker.
    device : Device
        Device identity or explicit inapplicability marker.
    driver_runtime : VersionedComponent
        Driver/runtime identity or explicit inapplicability marker.
    determinism : Determinism
        Determinism and randomness configuration.
    parallelism : Parallelism
        Resolved execution counts.
    active_preconditions : tuple[str, ...]
        Sorted active v0.1 preconditions.
    """

    compiler: RevisionedComponent
    runtime: RevisionedComponent
    target: Target
    backend: VersionedComponent
    device: Device
    driver_runtime: VersionedComponent
    determinism: Determinism
    parallelism: Parallelism
    active_preconditions: tuple[str, ...]

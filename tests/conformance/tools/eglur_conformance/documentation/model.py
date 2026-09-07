"""Canonical values for Markdown link parsing and validation."""

from dataclasses import dataclass
from pathlib import PurePosixPath


@dataclass(frozen=True)
class MarkdownSource:
    """One canonical tracked Markdown source.

    Parameters
    ----------
    path : pathlib.PurePosixPath
        Exact repository-relative Git path.
    text : str
        UTF-8-decoded worktree content.
    """

    path: PurePosixPath
    text: str


@dataclass(frozen=True)
class LocatedDestination:
    """One source-located Markdown link destination.

    Parameters
    ----------
    line : int
        One-based source line.
    column : int
        One-based destination column.
    destination : str
        Decoded Markdown destination text.
    """

    line: int
    column: int
    destination: str


@dataclass(frozen=True)
class UndefinedReference:
    """One source-located reference without a definition.

    Parameters
    ----------
    label : str
        Canonical CommonMark reference label.
    line : int
        One-based source line.
    column : int
        One-based reference column.
    """

    label: str
    line: int
    column: int


@dataclass(frozen=True)
class ParsedMarkdownLinks:
    """One Markdown document's local-link syntax projection.

    Parameters
    ----------
    path : pathlib.PurePosixPath
        Exact repository-relative source identity.
    anchors : frozenset[str]
        Deterministic ATX heading anchor identities.
    destinations : tuple[LocatedDestination, ...]
        Inline and used reference destinations.
    undefined_references : tuple[UndefinedReference, ...]
        Full or collapsed references without definitions.
    """

    path: PurePosixPath
    anchors: frozenset[str]
    destinations: tuple[LocatedDestination, ...]
    undefined_references: tuple[UndefinedReference, ...]


@dataclass(frozen=True)
class LinkViolation:
    """One source-located invalid Markdown destination.

    Parameters
    ----------
    path : pathlib.PurePosixPath
        Markdown source containing the destination.
    line : int
        One-based source line.
    column : int
        One-based destination or reference column.
    destination : str
        Destination text or unresolved reference label.
    reason : str
        Stable human-readable failure reason.
    """

    path: PurePosixPath
    line: int
    column: int
    destination: str
    reason: str

    def render(self) -> str:
        """Render the actionable source location and failure reason.

        Returns
        -------
        str
            Stable diagnostic text for CLI output.
        """
        return (
            f"{self.path.as_posix()}:{self.line}:{self.column}: "
            f"{self.reason}: {self.destination!r}"
        )

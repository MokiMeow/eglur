"""Canonical models for syntax-front-end expectation projection."""

import re
from dataclasses import dataclass
from enum import StrEnum
from pathlib import PurePosixPath
from typing import TypeAlias

from eglur_conformance.artifact_content import SCHEMA_VERSION
from eglur_conformance.diagnostics.model import ByteRange

_DIGEST_PATTERN = re.compile(r"sha256:[0-9a-f]{64}\Z")


class EntryPoint(StrEnum):
    """Closed parser entry points represented in the front-end corpus."""

    COMPILATION_UNIT = "compilation_unit"
    TOKENS = "tokens"
    EXPRESSION = "expression"
    EXPRESSION_SEQUENCE = "expression_sequence"
    CONTROL_HEAD = "control_head"


class OutcomeKind(StrEnum):
    """Closed front-end outcomes independent of later compiler phases."""

    SUCCESS = "success"
    LEXICAL_FAILURE = "lexical_failure"
    PARSE_FAILURE = "parse_failure"


def _require_text(value: str, context: str) -> None:
    if type(value) is not str:
        raise TypeError(f"{context} must be a string")
    if not value:
        raise ValueError(f"{context} must be non-empty")
    if value != value.strip():
        raise ValueError(f"{context} must have no outer whitespace")
    if any(0xD800 <= ord(character) <= 0xDFFF for character in value):
        raise ValueError(f"{context} must not contain lone surrogate code points")


def _require_relative_posix_path(value: str, context: str, suffix: str) -> None:
    _require_text(value, context)
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or path.as_posix() != value
        or "\\" in value
        or "\0" in value
        or ".." in path.parts
    ):
        raise ValueError(f"{context} must be a canonical relative POSIX path")
    if path.suffix != suffix:
        raise ValueError(f"{context} must end in {suffix}")


@dataclass(frozen=True)
class InlineConformanceOrigin:
    """Identify one inline manifest-owned conformance source."""

    suite_id: str
    test_id: str

    def __post_init__(self) -> None:
        """Reject incomplete conformance identity components."""
        _require_text(self.suite_id, "inline origin suite_id")
        _require_text(self.test_id, "inline origin test_id")


@dataclass(frozen=True)
class FileConformanceOrigin:
    """Identify one Eglur file inside a manifest-owned file-set fixture."""

    suite_id: str
    test_id: str
    path: str

    def __post_init__(self) -> None:
        """Reject incomplete identities and non-canonical source paths."""
        _require_text(self.suite_id, "file origin suite_id")
        _require_text(self.test_id, "file origin test_id")
        _require_relative_posix_path(self.path, "file origin path", ".eglur")


@dataclass(frozen=True)
class RegistryConformanceOrigin:
    """Identify one Eglur file from a complete registry acquisition."""

    suite_id: str
    test_id: str
    package: str
    version: str
    path: str

    def __post_init__(self) -> None:
        """Reject incomplete registry identity components."""
        _require_text(self.suite_id, "registry origin suite_id")
        _require_text(self.test_id, "registry origin test_id")
        _require_text(self.package, "registry origin package")
        _require_text(self.version, "registry origin version")
        _require_relative_posix_path(self.path, "registry origin path", ".eglur")


@dataclass(frozen=True)
class SyntaxCaseOrigin:
    """Identify one authored focused syntax case."""

    case_id: str

    def __post_init__(self) -> None:
        """Reject an incomplete syntax-case identity."""
        _require_text(self.case_id, "syntax-case origin case_id")


FrontendOrigin: TypeAlias = (
    InlineConformanceOrigin
    | FileConformanceOrigin
    | RegistryConformanceOrigin
    | SyntaxCaseOrigin
)


def origin_identity(origin: FrontendOrigin) -> tuple[str, ...]:
    """Return one globally comparable source-unit identity.

    Parameters
    ----------
    origin : FrontendOrigin
        Source provenance whose identity is required.

    Returns
    -------
    tuple[str, ...]
        Closed discriminant followed by identity components.
    """
    if isinstance(origin, InlineConformanceOrigin):
        return ("inline", origin.suite_id, origin.test_id)
    if isinstance(origin, FileConformanceOrigin):
        return ("file", origin.suite_id, origin.test_id, origin.path)
    if isinstance(origin, RegistryConformanceOrigin):
        return (
            "registry",
            origin.suite_id,
            origin.test_id,
            origin.package,
            origin.version,
            origin.path,
        )
    return ("syntax_case", origin.case_id)


@dataclass(frozen=True)
class Utf8Text:
    """Exact Unicode-scalar source text."""

    text: str

    def __post_init__(self) -> None:
        """Reject mistyped text and lone surrogate code points."""
        if type(self.text) is not str:
            raise TypeError("UTF-8 source text must be a string")
        if any(0xD800 <= ord(character) <= 0xDFFF for character in self.text):
            raise ValueError("UTF-8 source text must not contain lone surrogates")


@dataclass(frozen=True)
class RawBytes:
    """Exact source bytes, including invalid UTF-8 input."""

    content: bytes

    def __post_init__(self) -> None:
        """Require an immutable exact byte value."""
        if type(self.content) is not bytes:
            raise TypeError("raw source content must be bytes")


FrontendSource: TypeAlias = Utf8Text | RawBytes


@dataclass(frozen=True, order=True)
class PublicExample:
    """Locate a tracked public example linked to a conformance fixture."""

    document: str
    opening_line: int

    def __post_init__(self) -> None:
        """Reject non-canonical document paths and invalid line numbers."""
        _require_relative_posix_path(
            self.document,
            "public example document",
            ".md",
        )
        if type(self.opening_line) is not int:
            raise TypeError("public example opening_line must be an integer")
        if self.opening_line <= 0:
            raise ValueError("public example opening_line must be positive")


@dataclass(frozen=True)
class MarkerSpanExpectation:
    """Preserve one resolved marker-based primary-span assertion.

    Parameters
    ----------
    marker : str
        Fixture-local marker name.
    source : str
        Canonical fixture source-unit identity.
    line : int
        One-based logical source line.

    Raises
    ------
    TypeError
        A text field or `line` has the wrong scalar type.
    ValueError
        A text field is empty or malformed, or `line` is not positive.
    """

    marker: str
    source: str
    line: int

    def __post_init__(self) -> None:
        """Reject incomplete marker locations."""
        _require_text(self.marker, "primary span marker")
        _require_text(self.source, "primary span source")
        if type(self.line) is not int:
            raise TypeError("primary span line must be an integer")
        if self.line <= 0:
            raise ValueError("primary span line must be positive")


@dataclass(frozen=True)
class ByteSpanExpectation:
    """Preserve one exact physical source-byte assertion.

    Parameters
    ----------
    source : str
        Canonical fixture source-unit identity.
    byte_range : ByteRange
        Zero-based half-open physical byte range.

    Raises
    ------
    TypeError
        `source` is not text or `byte_range` is not a `ByteRange`.
    ValueError
        `source` is empty or malformed.
    """

    source: str
    byte_range: ByteRange

    def __post_init__(self) -> None:
        """Reject incomplete or mistyped byte-span assertions."""
        _require_text(self.source, "primary span source")
        if not isinstance(self.byte_range, ByteRange):
            raise TypeError("primary span byte_range must be a ByteRange")


SpanExpectation: TypeAlias = MarkerSpanExpectation | ByteSpanExpectation


@dataclass(frozen=True)
class FrontendExpectation:
    """One front-end outcome with an optional existing span assertion."""

    kind: OutcomeKind
    diagnostic_code: str | None
    primary_span: SpanExpectation | None

    def __post_init__(self) -> None:
        """Enforce the closed relationship between outcome and diagnostic code."""
        if not isinstance(self.kind, OutcomeKind):
            raise TypeError("front-end expectation kind must be an OutcomeKind")
        if self.kind is OutcomeKind.SUCCESS:
            if self.diagnostic_code is not None or self.primary_span is not None:
                raise ValueError("front-end success cannot carry a diagnostic")
            return

        if self.diagnostic_code is None:
            raise ValueError("front-end failure requires a diagnostic code")
        _require_text(self.diagnostic_code, "front-end diagnostic code")
        expected_prefix = (
            "LEX_" if self.kind is OutcomeKind.LEXICAL_FAILURE else "PARSE_"
        )
        if not self.diagnostic_code.startswith(expected_prefix):
            raise ValueError(
                f"{self.kind.value} diagnostic must start with {expected_prefix}"
            )
        if self.primary_span is not None and not isinstance(
            self.primary_span,
            (MarkerSpanExpectation, ByteSpanExpectation),
        ):
            raise TypeError("front-end primary_span has an unsupported assertion type")


@dataclass(frozen=True)
class FrontendCase:
    """One exact source unit and its selected front-end contract."""

    origin: FrontendOrigin
    entrypoint: EntryPoint
    source: FrontendSource
    expectation: FrontendExpectation
    public_examples: tuple[PublicExample, ...] = ()

    def __post_init__(self) -> None:
        """Reject invalid axis values and non-canonical public provenance."""
        origin_types = (
            InlineConformanceOrigin,
            FileConformanceOrigin,
            RegistryConformanceOrigin,
            SyntaxCaseOrigin,
        )
        if not isinstance(self.origin, origin_types):
            raise TypeError("front-end case origin has an unsupported type")
        if not isinstance(self.entrypoint, EntryPoint):
            raise TypeError("front-end case entrypoint must be an EntryPoint")
        if not isinstance(self.source, (Utf8Text, RawBytes)):
            raise TypeError("front-end case source has an unsupported type")
        if not isinstance(self.expectation, FrontendExpectation):
            raise TypeError("front-end case expectation is invalid")
        if type(self.public_examples) is not tuple or not all(
            isinstance(example, PublicExample) for example in self.public_examples
        ):
            raise TypeError("front-end public_examples must be a tuple of locations")
        if self.public_examples != tuple(sorted(set(self.public_examples))):
            raise ValueError("front-end public_examples must be unique and sorted")
        if self.public_examples and not isinstance(
            self.origin,
            InlineConformanceOrigin,
        ):
            raise ValueError("only inline conformance sources may have public examples")

    @property
    def identity(self) -> tuple[str, ...]:
        """Return the globally unique source-unit identity.

        Returns
        -------
        tuple[str, ...]
            Closed origin identity used for ordering and deduplication.
        """
        return origin_identity(self.origin)


@dataclass(frozen=True)
class FrontendCorpus:
    """One deterministic projection of every tracked front-end source unit."""

    schema_version: str
    input_digest: str
    cases: tuple[FrontendCase, ...]

    def __post_init__(self) -> None:
        """Require a current schema, exact digest, and sorted unique identities."""
        if self.schema_version != SCHEMA_VERSION:
            raise ValueError(f"front-end schema_version must be {SCHEMA_VERSION!r}")
        if (
            type(self.input_digest) is not str
            or _DIGEST_PATTERN.fullmatch(self.input_digest) is None
        ):
            raise ValueError("front-end input_digest must be a SHA-256 address")
        if type(self.cases) is not tuple or not all(
            isinstance(case, FrontendCase) for case in self.cases
        ):
            raise TypeError("front-end cases must be a tuple of FrontendCase values")
        identities = tuple(case.identity for case in self.cases)
        if identities != tuple(sorted(identities)):
            raise ValueError("front-end case identities must be sorted")
        if len(set(identities)) != len(identities):
            raise ValueError("front-end case identities must be globally unique")


@dataclass(frozen=True)
class SyntaxCase:
    """One authored source fragment with a focused front-end contract."""

    case_id: str
    entrypoint: EntryPoint
    source: str
    expectation: FrontendExpectation

    def __post_init__(self) -> None:
        """Reject malformed focused syntax inputs before projection."""
        _require_text(self.case_id, "syntax case id")
        if not isinstance(self.entrypoint, EntryPoint):
            raise TypeError("syntax case entrypoint must be an EntryPoint")
        if self.entrypoint is EntryPoint.COMPILATION_UNIT:
            raise ValueError("syntax cases must use a focused front-end entrypoint")
        if type(self.source) is not str:
            raise TypeError("syntax case source must be a string")
        if not self.source:
            raise ValueError("syntax case source must be non-empty")
        if any(0xD800 <= ord(character) <= 0xDFFF for character in self.source):
            raise ValueError("syntax case source must not contain lone surrogates")
        if not isinstance(self.expectation, FrontendExpectation):
            raise TypeError("syntax case expectation must be a FrontendExpectation")
        if (
            self.entrypoint is EntryPoint.TOKENS
            and self.expectation.kind is OutcomeKind.PARSE_FAILURE
        ):
            raise ValueError("token entrypoint cannot carry a parser failure")


@dataclass(frozen=True)
class PublicExampleBinding:
    """Boundary evidence linking one public block to exact fixture source."""

    fixture_id: str
    document: str
    opening_line: int
    source: str

    def __post_init__(self) -> None:
        """Reject malformed public-example metadata before projection."""
        _require_text(self.fixture_id, "public fixture id")
        _require_relative_posix_path(
            self.document,
            "public fixture document",
            ".md",
        )
        if type(self.opening_line) is not int:
            raise TypeError("public fixture opening_line must be an integer")
        if self.opening_line <= 0:
            raise ValueError("public fixture opening_line must be positive")
        if type(self.source) is not str:
            raise TypeError("public fixture source must be a string")
        if any(0xD800 <= ord(character) <= 0xDFFF for character in self.source):
            raise ValueError("public fixture source must not contain lone surrogates")

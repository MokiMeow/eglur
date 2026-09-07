"""Canonical diagnostic channel and coverage models."""

from dataclasses import dataclass
from typing import Literal, TypeAlias

DiagnosticChannel: TypeAlias = Literal[
    "compile_error",
    "runtime_error",
    "value_diagnostics",
]
CoveragePolicy: TypeAlias = Literal[
    "hard_required_asserted",
    "conditional_required",
    "optional_emit_if_present",
    "excluded_nonbaseline",
]
DiagnosticScalar: TypeAlias = str | int | bool

I64_MIN = -(2**63)
I64_MAX = 2**63 - 1

COMPILE_ERROR: DiagnosticChannel = "compile_error"
RUNTIME_ERROR: DiagnosticChannel = "runtime_error"
VALUE_DIAGNOSTICS: DiagnosticChannel = "value_diagnostics"
CHANNELS = frozenset({COMPILE_ERROR, RUNTIME_ERROR, VALUE_DIAGNOSTICS})

HARD_REQUIRED_ASSERTED: CoveragePolicy = "hard_required_asserted"
CONDITIONAL_REQUIRED: CoveragePolicy = "conditional_required"
OPTIONAL_EMIT_IF_PRESENT: CoveragePolicy = "optional_emit_if_present"
EXCLUDED_NONBASELINE: CoveragePolicy = "excluded_nonbaseline"
COVERAGE_POLICIES = frozenset(
    {
        HARD_REQUIRED_ASSERTED,
        CONDITIONAL_REQUIRED,
        OPTIONAL_EMIT_IF_PRESENT,
        EXCLUDED_NONBASELINE,
    }
)


@dataclass(frozen=True)
class DiagnosticChannelProfile:
    """Output channels in which one diagnostic code may appear."""

    allowed_channels: frozenset[DiagnosticChannel]


@dataclass(frozen=True)
class DiagnosticCoverageObligation:
    """Baseline coverage policy for one diagnostic code."""

    policy: CoveragePolicy
    precondition: str | None


@dataclass(frozen=True)
class DiagnosticProfile:
    """One code's independent output and conformance obligations."""

    channels: DiagnosticChannelProfile
    coverage: DiagnosticCoverageObligation


def _require_non_empty_scalar_text(value: str, field: str) -> None:
    if not isinstance(value, str) or value == "":
        raise ValueError(f"{field} must be a non-empty string")
    if any(0xD800 <= ord(character) <= 0xDFFF for character in value):
        raise ValueError(f"{field} must contain only Unicode scalar values")


def _require_i64(value: int, field: str, *, positive: bool) -> None:
    lower_bound = 1 if positive else 0
    description = "positive" if positive else "non-negative"
    if type(value) is not int or value < lower_bound or value > I64_MAX:
        raise ValueError(f"{field} must be a {description} i64")


@dataclass(frozen=True)
class ByteRange:
    """One canonical half-open physical source-byte range.

    Parameters
    ----------
    start : int
        Zero-based inclusive physical byte offset.
    end : int
        Zero-based exclusive physical byte offset.

    Raises
    ------
    ValueError
        Either offset is not a non-negative `i64`, or `end` precedes `start`.
    """

    start: int
    end: int

    def __post_init__(self) -> None:
        _require_i64(self.start, "start", positive=False)
        _require_i64(self.end, "end", positive=False)
        if self.end < self.start:
            raise ValueError("byte-range end must not precede start")


@dataclass(frozen=True)
class SourceCoordinate:
    """One logical one-based Unicode-scalar source coordinate.

    Parameters
    ----------
    line : int
        One-based logical line.
    column : int
        One-based Unicode-scalar column.

    Raises
    ------
    ValueError
        Either coordinate is not a positive `i64`.
    """

    line: int
    column: int

    def __post_init__(self) -> None:
        _require_i64(self.line, "line", positive=True)
        _require_i64(self.column, "column", positive=True)


@dataclass(frozen=True)
class TextRange:
    """Derived logical coordinates for both ends of a source span.

    Parameters
    ----------
    start : SourceCoordinate
        Inclusive logical start coordinate.
    end : SourceCoordinate
        Exclusive logical end coordinate.

    Raises
    ------
    TypeError
        Either endpoint is not a `SourceCoordinate`.
    ValueError
        The end coordinate precedes the start coordinate.
    """

    start: SourceCoordinate
    end: SourceCoordinate

    def __post_init__(self) -> None:
        if not isinstance(self.start, SourceCoordinate):
            raise TypeError("text-range start must be a SourceCoordinate")
        if not isinstance(self.end, SourceCoordinate):
            raise TypeError("text-range end must be a SourceCoordinate")
        if (self.end.line, self.end.column) < (
            self.start.line,
            self.start.column,
        ):
            raise ValueError("text-range end must not precede start")


@dataclass(frozen=True)
class SourceSpan:
    """Canonical physical source span with an optional text projection.

    Parameters
    ----------
    source : str
        Non-empty source-unit identity.
    byte_range : ByteRange
        Canonical zero-based half-open physical byte range.
    text_range : TextRange or None
        Derived logical coordinate projection when the source permits one.

    Raises
    ------
    TypeError
        `byte_range` or `text_range` has the wrong model type.
    ValueError
        `source` is empty or contains a lone surrogate.
    """

    source: str
    byte_range: ByteRange
    text_range: TextRange | None = None

    def __post_init__(self) -> None:
        _require_non_empty_scalar_text(self.source, "source")
        if not isinstance(self.byte_range, ByteRange):
            raise TypeError("source-span byte_range must be a ByteRange")
        if self.text_range is not None and not isinstance(self.text_range, TextRange):
            raise TypeError("source-span text_range must be a TextRange or None")

    @property
    def kind(self) -> Literal["source"]:
        """Return the serialized primary-span variant.

        Returns
        -------
        Literal["source"]
            Source-bound span discriminator.
        """
        return "source"

    def source_order_key(self) -> tuple[str, int, int]:
        """Return the canonical source ordering key.

        Returns
        -------
        tuple[str, int, int]
            Source identity, byte start, and byte end.
        """
        return self.source, self.byte_range.start, self.byte_range.end


@dataclass(frozen=True)
class UnavailableSpan:
    """A diagnostic without a source-bound primary span."""

    @property
    def kind(self) -> Literal["unavailable"]:
        """Return the serialized primary-span variant.

        Returns
        -------
        Literal["unavailable"]
            Source-unavailable span discriminator.
        """
        return "unavailable"


PrimarySpan: TypeAlias = SourceSpan | UnavailableSpan


@dataclass(frozen=True)
class RelatedSpan:
    """One validated secondary source span in a diagnostic record.

    Parameters
    ----------
    span : SourceSpan
        Canonical source-bound span.
    label : str
        Non-empty explanatory label.

    Raises
    ------
    TypeError
        `span` is not a `SourceSpan`.
    ValueError
        `label` is empty or contains a lone surrogate.
    """

    span: SourceSpan
    label: str

    def __post_init__(self) -> None:
        if not isinstance(self.span, SourceSpan):
            raise TypeError("related span must contain a SourceSpan")
        _require_non_empty_scalar_text(self.label, "label")

    def source_order_key(self) -> tuple[str, int, int, str]:
        """Return the canonical default ordering key.

        Returns
        -------
        tuple[str, int, int, str]
            Source span order followed by the related label.
        """
        return (*self.span.source_order_key(), self.label)


DiagnosticRegistry: TypeAlias = dict[str, DiagnosticProfile]

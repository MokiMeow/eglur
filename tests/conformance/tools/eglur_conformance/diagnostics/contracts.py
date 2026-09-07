"""Load the normative machine-readable diagnostic field contract."""

import re
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from eglur_conformance.diagnostics.model import (
    I64_MAX,
    I64_MIN,
    ByteRange,
    DiagnosticScalar,
    RelatedSpan,
    SourceCoordinate,
    SourceSpan,
    TextRange,
)
from eglur_conformance.yaml_document import YamlValue, load_yaml

DEFAULT_CONTRACT_PATH = Path("docs/spec/errors/diagnostic-contracts.yaml")
RE_CODE = re.compile(r"^[A-Z][A-Z0-9_]+$")
SEVERITIES = frozenset({"error", "warning", "note"})
SCALAR_KINDS = frozenset({"bool", "int", "str"})
_RE_RELATED_EXAMPLE_ID = re.compile(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
_SOURCE_REQUIRED_FIELDS = (
    ("kind", "source"),
    ("source", "Str"),
    ("bytes", "ByteRange"),
)
_SOURCE_OPTIONAL_FIELDS = (("text", "TextRange"),)
_BYTE_RANGE_REQUIRED_FIELDS = (("start", "i64"), ("end", "i64"))
_TEXT_RANGE_REQUIRED_FIELDS = (
    ("start", "SourceCoordinate"),
    ("end", "SourceCoordinate"),
)
_COORDINATE_REQUIRED_FIELDS = (("line", "i64"), ("column", "i64"))
_SOURCE_ORDER_KEYS = ("source", "bytes.start", "bytes.end")
_PRIMARY_SOURCE_TYPE = "SourceSpan"
_PRIMARY_UNAVAILABLE_KIND = "unavailable"
_RELATED_REQUIRED_FIELDS = (
    ("span", "SourceSpan"),
    ("label", "Str"),
)
_RELATED_SOURCE_KEYS = (
    "span.source",
    "span.bytes.start",
    "span.bytes.end",
    "label",
)


def diagnostic_scalar_kind(value: DiagnosticScalar) -> str:
    """Return the stable kind tag for one normalized diagnostic scalar."""
    if type(value) is bool:
        return "bool"
    if type(value) is int:
        return "int"
    if type(value) is str:
        return "str"
    raise TypeError("diagnostic scalar must already be normalized")


def expect_diagnostic_scalar(value: YamlValue, context: str) -> DiagnosticScalar:
    """Normalize one exact Str/i64/Bool diagnostic scalar."""
    if type(value) is bool:
        return cast(DiagnosticScalar, value)
    if type(value) is str:
        if any(0xD800 <= ord(character) <= 0xDFFF for character in value):
            raise ValueError(f"{context} must not contain lone surrogate code points")
        return value
    if type(value) is int:
        if value < I64_MIN or value > I64_MAX:
            raise ValueError(f"{context} must fit signed i64")
        return value
    raise TypeError(f"{context} must be Str, i64, or Bool")


def is_diagnostic_scalar(value: YamlValue) -> bool:
    """Return whether one value belongs to the stable scalar domain."""
    try:
        expect_diagnostic_scalar(value, "diagnostic scalar")
    except (TypeError, ValueError):
        return False
    return True


def diagnostic_scalars_equal(
    left: DiagnosticScalar,
    right: DiagnosticScalar,
) -> bool:
    """Compare both stable scalar kind and value."""
    return (
        diagnostic_scalar_kind(left) == diagnostic_scalar_kind(right) and left == right
    )


@dataclass(frozen=True)
class MessageTemplate:
    """One contextual message shape bound to diagnostic data fields."""

    pattern: str
    fields: tuple[str, ...]


@dataclass(frozen=True)
class ConditionalDataRequirement:
    """Data keys required when one exact fixture value activates a condition."""

    key: str
    equals: DiagnosticScalar
    required: tuple[str, ...]

    def is_active(self, data_equals: dict[str, DiagnosticScalar]) -> bool:
        """Return whether fixture values activate this requirement."""
        return self.key in data_equals and diagnostic_scalars_equal(
            data_equals[self.key],
            self.equals,
        )


@dataclass(frozen=True)
class DiagnosticContract:
    """Normative field contract for one diagnostic code."""

    severity: str
    message_all_of: tuple[str, ...]
    message_any_of: tuple[tuple[str, ...], ...]
    message_templates: tuple[MessageTemplate, ...]
    message_contextual: tuple[str, ...]
    data_required: tuple[str, ...]
    data_required_if_emitted: tuple[str, ...]
    data_conditional_required: tuple[ConditionalDataRequirement, ...]
    data_exact_value_keys: tuple[str, ...]

    def required_data_keys(
        self,
        data_equals: dict[str, DiagnosticScalar],
    ) -> tuple[str, ...]:
        """Return unconditional and active conditional required data keys."""
        keys = list(self.data_required)
        for condition in self.data_conditional_required:
            if condition.is_active(data_equals):
                keys.extend(condition.required)
        return tuple(dict.fromkeys(keys))


@dataclass(frozen=True)
class ByteRangeContract:
    """Machine contract for canonical physical byte ranges.

    Parameters
    ----------
    required_fields : tuple[tuple[str, str], ...]
        Closed byte-range field schema.
    start_base : int
        Required physical-byte offset base.
    unit : str
        Required byte-coordinate unit.
    end_semantics : str
        Required range-end convention.
    """

    required_fields: tuple[tuple[str, str], ...]
    start_base: int
    unit: str
    end_semantics: str


@dataclass(frozen=True)
class SourceCoordinateContract:
    """Machine contract for derived logical source coordinates.

    Parameters
    ----------
    required_fields : tuple[tuple[str, str], ...]
        Closed coordinate field schema.
    line_base : int
        Required line-number base.
    column_base : int
        Required column-number base.
    column_unit : str
        Required logical column unit.
    """

    required_fields: tuple[tuple[str, str], ...]
    line_base: int
    column_base: int
    column_unit: str


@dataclass(frozen=True)
class TextRangeContract:
    """Machine contract for a source span's text projection.

    Parameters
    ----------
    required_fields : tuple[tuple[str, str], ...]
        Closed text-range field schema.
    coordinate : SourceCoordinateContract
        Coordinate representation contract.
    leading_utf8_bom : str
        Leading-BOM treatment.
    line_endings : str
        Logical line-ending projection.
    end_semantics : str
        Required range-end convention.
    """

    required_fields: tuple[tuple[str, str], ...]
    coordinate: SourceCoordinateContract
    leading_utf8_bom: str
    line_endings: str
    end_semantics: str


@dataclass(frozen=True)
class SourceSpanContract:
    """Global canonical source-span schema and examples.

    Parameters
    ----------
    required_fields : tuple[tuple[str, str], ...]
        Closed required field schema.
    optional_fields : tuple[tuple[str, str], ...]
        Closed optional field schema.
    byte_range : ByteRangeContract
        Canonical physical range contract.
    text_range : TextRangeContract
        Optional logical projection contract.
    source_keys : tuple[str, ...]
        Deterministic source-order keys.
    examples : tuple[SourceSpan, ...]
        Validated normative examples.
    """

    required_fields: tuple[tuple[str, str], ...]
    optional_fields: tuple[tuple[str, str], ...]
    byte_range: ByteRangeContract
    text_range: TextRangeContract
    source_keys: tuple[str, ...]
    examples: tuple[SourceSpan, ...]


@dataclass(frozen=True)
class PrimarySpanContract:
    """Closed primary-span variant contract.

    Parameters
    ----------
    source_type : str
        Source-bound variant model name.
    unavailable_kind : str
        Source-unavailable variant discriminator.
    """

    source_type: str
    unavailable_kind: str


@dataclass(frozen=True)
class RelatedSpanOrdering:
    """Default and code-specific ordering policy for related spans.

    Parameters
    ----------
    default : str
        Default ordering policy name.
    code_specific_override : bool
        Whether normative code-specific order may override the default.
    source_keys : tuple[str, ...]
        Default source-order keys.
    """

    default: str
    code_specific_override: bool
    source_keys: tuple[str, ...]


@dataclass(frozen=True)
class RelatedSpanExample:
    """One machine-validated normative related-span sequence.

    Parameters
    ----------
    example_id : str
        Unique lowercase kebab-case example identity.
    spans : tuple[RelatedSpan, ...]
        Ordered related-span sequence.
    """

    example_id: str
    spans: tuple[RelatedSpan, ...]


@dataclass(frozen=True)
class RelatedSpanContract:
    """Global emitted-record contract for the optional related field.

    Parameters
    ----------
    required_fields : tuple[tuple[str, str], ...]
        Closed related-span field schema.
    ordering : RelatedSpanOrdering
        Default and override ordering rules.
    examples : tuple[RelatedSpanExample, ...]
        Validated normative examples.
    """

    required_fields: tuple[tuple[str, str], ...]
    ordering: RelatedSpanOrdering
    examples: tuple[RelatedSpanExample, ...]


@dataclass(frozen=True)
class DiagnosticContracts:
    """The global diagnostic record schema and per-code contracts.

    Parameters
    ----------
    source_span : SourceSpanContract
        Canonical source-span contract.
    primary_span : PrimarySpanContract
        Closed primary-span variant contract.
    related_span : RelatedSpanContract
        Optional related-span contract.
    codes : dict[str, DiagnosticContract]
        Per-code diagnostic contracts.
    """

    source_span: SourceSpanContract
    primary_span: PrimarySpanContract
    related_span: RelatedSpanContract
    codes: dict[str, DiagnosticContract]


def expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    """Return a string-keyed mapping or raise a context-rich error."""
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def expect_list(value: YamlValue, context: str) -> list[YamlValue]:
    """Return a list or raise a context-rich error."""
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def expect_str(value: YamlValue, context: str) -> str:
    """Return a non-empty string or raise a context-rich error."""
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if value == "":
        raise ValueError(f"{context} must be a non-empty string")
    return value


def expect_exact_keys(
    mapping: dict[str, YamlValue],
    expected: set[str],
    context: str,
) -> None:
    """Require an exact mapping key set."""
    if set(mapping) != expected:
        raise ValueError(
            f"{context} keys must be {sorted(expected)}, found {sorted(mapping)}"
        )


def expect_str_tuple(value: YamlValue, context: str) -> tuple[str, ...]:
    """Return a duplicate-free tuple of non-empty strings."""
    items = tuple(
        expect_str(item, f"{context}[{index}]")
        for index, item in enumerate(expect_list(value, context))
    )
    if len(set(items)) != len(items):
        raise ValueError(f"{context} must not contain duplicates")
    return items


def parse_message(
    value: YamlValue,
    context: str,
) -> tuple[
    tuple[str, ...],
    tuple[tuple[str, ...], ...],
    tuple[MessageTemplate, ...],
    tuple[str, ...],
]:
    """Parse one message contract."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"all_of", "any_of", "templates", "contextual"},
        context,
    )
    all_of = expect_str_tuple(mapping["all_of"], f"{context}.all_of")
    any_of = tuple(
        expect_str_tuple(group, f"{context}.any_of[{index}]")
        for index, group in enumerate(
            expect_list(mapping["any_of"], f"{context}.any_of")
        )
    )
    if any(not group for group in any_of):
        raise ValueError(f"{context}.any_of groups must be non-empty")

    templates: list[MessageTemplate] = []
    for index, raw_template in enumerate(
        expect_list(mapping["templates"], f"{context}.templates")
    ):
        template_context = f"{context}.templates[{index}]"
        template = expect_mapping(raw_template, template_context)
        expect_exact_keys(template, {"pattern", "fields"}, template_context)
        fields = expect_str_tuple(template["fields"], f"{template_context}.fields")
        if not fields:
            raise ValueError(f"{template_context}.fields must be non-empty")
        pattern = expect_str(template["pattern"], f"{template_context}.pattern")
        for field in fields:
            if f"{{{field}}}" not in pattern:
                raise ValueError(f"{template_context}.pattern must contain {{{field}}}")
        templates.append(MessageTemplate(pattern=pattern, fields=fields))

    contextual = expect_str_tuple(mapping["contextual"], f"{context}.contextual")
    if not (all_of or any_of or templates or contextual):
        raise ValueError(f"{context} must define at least one message requirement")
    return all_of, any_of, tuple(templates), contextual


def parse_data(
    value: YamlValue,
    context: str,
) -> tuple[
    tuple[str, ...],
    tuple[str, ...],
    tuple[ConditionalDataRequirement, ...],
    tuple[str, ...],
]:
    """Parse one data contract."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {
            "required",
            "required_if_emitted",
            "conditional_required",
            "exact_value_keys",
        },
        context,
    )
    required = expect_str_tuple(mapping["required"], f"{context}.required")
    required_if_emitted = expect_str_tuple(
        mapping["required_if_emitted"],
        f"{context}.required_if_emitted",
    )
    overlap = set(required) & set(required_if_emitted)
    if overlap:
        raise ValueError(
            f"{context} keys cannot be both required and required-if-emitted: "
            f"{sorted(overlap)}"
        )

    conditional: list[ConditionalDataRequirement] = []
    for index, raw_condition in enumerate(
        expect_list(mapping["conditional_required"], f"{context}.conditional_required")
    ):
        condition_context = f"{context}.conditional_required[{index}]"
        condition = expect_mapping(raw_condition, condition_context)
        expect_exact_keys(condition, {"when", "required"}, condition_context)
        when = expect_mapping(condition["when"], f"{condition_context}.when")
        expect_exact_keys(when, {"key", "equals"}, f"{condition_context}.when")
        equals = expect_diagnostic_scalar(
            when["equals"],
            f"{condition_context}.when.equals",
        )
        condition_required = expect_str_tuple(
            condition["required"], f"{condition_context}.required"
        )
        if not condition_required:
            raise ValueError(f"{condition_context}.required must be non-empty")
        conditional.append(
            ConditionalDataRequirement(
                key=expect_str(when["key"], f"{condition_context}.when.key"),
                equals=equals,
                required=condition_required,
            )
        )

    exact_value_keys = expect_str_tuple(
        mapping["exact_value_keys"], f"{context}.exact_value_keys"
    )
    assertion_keys = (
        set(required)
        | set(required_if_emitted)
        | {condition.key for condition in conditional}
        | {key for condition in conditional for key in condition.required}
    )
    invalid_exact_keys = set(exact_value_keys) - assertion_keys
    if invalid_exact_keys:
        raise ValueError(
            f"{context}.exact_value_keys are not declared data keys: "
            f"{sorted(invalid_exact_keys)}"
        )
    return required, required_if_emitted, tuple(conditional), exact_value_keys


def _parse_field_types(
    value: YamlValue,
    expected: tuple[tuple[str, str], ...],
    context: str,
) -> tuple[tuple[str, str], ...]:
    """Validate one exact machine-contract field/type partition."""
    mapping = expect_mapping(value, context)
    expected_mapping = dict(expected)
    normalized = {
        key: expect_str(field_type, f"{context}.{key}")
        for key, field_type in mapping.items()
    }
    if normalized != expected_mapping:
        raise ValueError(
            f"{context} fields must be {expected_mapping}, found {normalized}"
        )
    return expected


def _parse_non_negative_i64(value: YamlValue, context: str) -> int:
    if type(value) is not int or value < 0 or value > I64_MAX:
        raise ValueError(f"{context} must be a non-negative i64")
    return value


def _parse_positive_i64(value: YamlValue, context: str) -> int:
    if type(value) is not int or value <= 0 or value > I64_MAX:
        raise ValueError(f"{context} must be a positive i64")
    return value


def _parse_byte_range(value: YamlValue, context: str) -> ByteRange:
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"start", "end"}, context)
    try:
        return ByteRange(
            start=_parse_non_negative_i64(mapping["start"], f"{context}.start"),
            end=_parse_non_negative_i64(mapping["end"], f"{context}.end"),
        )
    except ValueError as error:
        raise ValueError(f"{context}.{error}") from error


def _parse_source_coordinate(value: YamlValue, context: str) -> SourceCoordinate:
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"line", "column"}, context)
    return SourceCoordinate(
        line=_parse_positive_i64(mapping["line"], f"{context}.line"),
        column=_parse_positive_i64(mapping["column"], f"{context}.column"),
    )


def _parse_text_range(value: YamlValue, context: str) -> TextRange:
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"start", "end"}, context)
    try:
        return TextRange(
            start=_parse_source_coordinate(mapping["start"], f"{context}.start"),
            end=_parse_source_coordinate(mapping["end"], f"{context}.end"),
        )
    except ValueError as error:
        raise ValueError(f"{context}.{error}") from error


def _parse_source_span(value: YamlValue, context: str) -> SourceSpan:
    mapping = expect_mapping(value, context)
    required = {field for field, _ in _SOURCE_REQUIRED_FIELDS}
    allowed = required | {field for field, _ in _SOURCE_OPTIONAL_FIELDS}
    missing = sorted(required - set(mapping))
    unknown = sorted(set(mapping) - allowed)
    if missing or unknown:
        raise ValueError(
            f"{context} field mismatch: missing={missing}, unknown={unknown}"
        )
    if mapping["kind"] != "source":
        raise ValueError(f"{context}.kind must be 'source'")
    try:
        return SourceSpan(
            source=expect_str(mapping["source"], f"{context}.source"),
            byte_range=_parse_byte_range(mapping["bytes"], f"{context}.bytes"),
            text_range=(
                None
                if "text" not in mapping
                else _parse_text_range(mapping["text"], f"{context}.text")
            ),
        )
    except (TypeError, ValueError) as error:
        raise ValueError(f"{context}.{error}") from error


def _parse_byte_range_contract(
    value: YamlValue,
    context: str,
) -> ByteRangeContract:
    """Parse the fixed physical-byte range contract."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"fields", "start_base", "unit", "end_semantics"},
        context,
    )
    fields = expect_mapping(mapping["fields"], f"{context}.fields")
    expect_exact_keys(fields, {"required"}, f"{context}.fields")
    if type(mapping["start_base"]) is not int or mapping["start_base"] != 0:
        raise ValueError(f"{context}.start_base must be 0")
    if mapping["unit"] != "physical_byte":
        raise ValueError(f"{context}.unit must be 'physical_byte'")
    if mapping["end_semantics"] != "half_open":
        raise ValueError(f"{context}.end_semantics must be 'half_open'")
    return ByteRangeContract(
        required_fields=_parse_field_types(
            fields["required"],
            _BYTE_RANGE_REQUIRED_FIELDS,
            f"{context}.fields.required",
        ),
        start_base=0,
        unit="physical_byte",
        end_semantics="half_open",
    )


def _parse_source_coordinate_contract(
    value: YamlValue,
    context: str,
) -> SourceCoordinateContract:
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"fields", "line_base", "column_base", "column_unit"},
        context,
    )
    fields = expect_mapping(mapping["fields"], f"{context}.fields")
    expect_exact_keys(fields, {"required"}, f"{context}.fields")
    if type(mapping["line_base"]) is not int or mapping["line_base"] != 1:
        raise ValueError(f"{context}.line_base must be 1")
    if type(mapping["column_base"]) is not int or mapping["column_base"] != 1:
        raise ValueError(f"{context}.column_base must be 1")
    if mapping["column_unit"] != "unicode_scalar":
        raise ValueError(f"{context}.column_unit must be 'unicode_scalar'")
    return SourceCoordinateContract(
        required_fields=_parse_field_types(
            fields["required"],
            _COORDINATE_REQUIRED_FIELDS,
            f"{context}.fields.required",
        ),
        line_base=1,
        column_base=1,
        column_unit="unicode_scalar",
    )


def _parse_text_range_contract(value: YamlValue, context: str) -> TextRangeContract:
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"fields", "coordinate", "normalization", "end_semantics"},
        context,
    )
    fields = expect_mapping(mapping["fields"], f"{context}.fields")
    expect_exact_keys(fields, {"required"}, f"{context}.fields")
    normalization = expect_mapping(
        mapping["normalization"],
        f"{context}.normalization",
    )
    expect_exact_keys(
        normalization,
        {"leading_utf8_bom", "line_endings"},
        f"{context}.normalization",
    )
    if normalization["leading_utf8_bom"] != "ignored":
        raise ValueError(f"{context}.normalization.leading_utf8_bom must be 'ignored'")
    if normalization["line_endings"] != "logical_lf":
        raise ValueError(f"{context}.normalization.line_endings must be 'logical_lf'")
    if mapping["end_semantics"] != "half_open":
        raise ValueError(f"{context}.end_semantics must be 'half_open'")
    return TextRangeContract(
        required_fields=_parse_field_types(
            fields["required"],
            _TEXT_RANGE_REQUIRED_FIELDS,
            f"{context}.fields.required",
        ),
        coordinate=_parse_source_coordinate_contract(
            mapping["coordinate"],
            f"{context}.coordinate",
        ),
        leading_utf8_bom="ignored",
        line_endings="logical_lf",
        end_semantics="half_open",
    )


def _parse_source_span_examples(
    value: YamlValue,
    context: str,
) -> tuple[SourceSpan, ...]:
    examples: list[SourceSpan] = []
    seen_ids: set[str] = set()
    for index, raw_example in enumerate(expect_list(value, context)):
        example_context = f"{context}[{index}]"
        example = expect_mapping(raw_example, example_context)
        expect_exact_keys(example, {"id", "span"}, example_context)
        example_id = expect_str(example["id"], f"{example_context}.id")
        if _RE_RELATED_EXAMPLE_ID.fullmatch(example_id) is None:
            raise ValueError(f"{example_context}.id must be lowercase kebab case")
        if example_id in seen_ids:
            raise ValueError(f"{example_context}.id duplicates {example_id!r}")
        seen_ids.add(example_id)
        examples.append(_parse_source_span(example["span"], f"{example_context}.span"))
    if not examples:
        raise ValueError(f"{context} must not be empty")
    return tuple(examples)


def _parse_source_span_contract(
    value: YamlValue,
    context: str,
) -> SourceSpanContract:
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"fields", "byte_range", "text_range", "ordering", "examples"},
        context,
    )
    fields = expect_mapping(mapping["fields"], f"{context}.fields")
    expect_exact_keys(fields, {"required", "optional"}, f"{context}.fields")
    ordering = expect_mapping(mapping["ordering"], f"{context}.ordering")
    expect_exact_keys(ordering, {"source_keys"}, f"{context}.ordering")
    source_keys = expect_str_tuple(
        ordering["source_keys"],
        f"{context}.ordering.source_keys",
    )
    if source_keys != _SOURCE_ORDER_KEYS:
        raise ValueError(f"{context}.source_keys must be {list(_SOURCE_ORDER_KEYS)}")
    return SourceSpanContract(
        required_fields=_parse_field_types(
            fields["required"],
            _SOURCE_REQUIRED_FIELDS,
            f"{context}.fields.required",
        ),
        optional_fields=_parse_field_types(
            fields["optional"],
            _SOURCE_OPTIONAL_FIELDS,
            f"{context}.fields.optional",
        ),
        byte_range=_parse_byte_range_contract(
            mapping["byte_range"],
            f"{context}.byte_range",
        ),
        text_range=_parse_text_range_contract(
            mapping["text_range"],
            f"{context}.text_range",
        ),
        source_keys=source_keys,
        examples=_parse_source_span_examples(
            mapping["examples"],
            f"{context}.examples",
        ),
    )


def _parse_primary_span_contract(
    value: YamlValue,
    context: str,
) -> PrimarySpanContract:
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"variants"}, context)
    variants = expect_mapping(mapping["variants"], f"{context}.variants")
    expect_exact_keys(variants, {"source", "unavailable"}, f"{context}.variants")
    source_type = expect_str(variants["source"], f"{context}.variants.source")
    if source_type != _PRIMARY_SOURCE_TYPE:
        raise ValueError(f"{context}.variants.source must be {_PRIMARY_SOURCE_TYPE!r}")
    unavailable = expect_mapping(
        variants["unavailable"],
        f"{context}.variants.unavailable",
    )
    expect_exact_keys(unavailable, {"kind"}, f"{context}.variants.unavailable")
    unavailable_kind = expect_str(
        unavailable["kind"],
        f"{context}.variants.unavailable.kind",
    )
    if unavailable_kind != _PRIMARY_UNAVAILABLE_KIND:
        raise ValueError(
            f"{context}.variants.unavailable.kind must be {_PRIMARY_UNAVAILABLE_KIND!r}"
        )
    return PrimarySpanContract(
        source_type=source_type,
        unavailable_kind=unavailable_kind,
    )


def _parse_related_ordering(
    value: YamlValue,
    context: str,
) -> RelatedSpanOrdering:
    """Parse the default source order and normative override boundary."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"default", "code_specific_override", "source_keys"},
        context,
    )
    if mapping["default"] != "source":
        raise ValueError(f"{context}.default must be 'source'")
    if (
        type(mapping["code_specific_override"]) is not bool
        or not mapping["code_specific_override"]
    ):
        raise ValueError(f"{context}.code_specific_override must be true")
    source_keys = expect_str_tuple(mapping["source_keys"], f"{context}.source_keys")
    if source_keys != _RELATED_SOURCE_KEYS:
        raise ValueError(f"{context}.source_keys must be {list(_RELATED_SOURCE_KEYS)}")
    return RelatedSpanOrdering(
        default="source",
        code_specific_override=True,
        source_keys=source_keys,
    )


def _parse_related_span(value: YamlValue, context: str) -> RelatedSpan:
    """Parse one exact RelatedSpan example value."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"span", "label"}, context)
    try:
        return RelatedSpan(
            span=_parse_source_span(mapping["span"], f"{context}.span"),
            label=expect_str(mapping["label"], f"{context}.label"),
        )
    except (TypeError, ValueError) as error:
        raise ValueError(f"{context}.{error}") from error


def _parse_related_examples(
    value: YamlValue,
    context: str,
) -> tuple[RelatedSpanExample, ...]:
    """Parse unique, non-empty, default-ordered example sequences."""
    examples: list[RelatedSpanExample] = []
    seen_ids: set[str] = set()
    for index, raw_example in enumerate(expect_list(value, context)):
        example_context = f"{context}[{index}]"
        example = expect_mapping(raw_example, example_context)
        expect_exact_keys(example, {"id", "spans"}, example_context)
        example_id = expect_str(example["id"], f"{example_context}.id")
        if _RE_RELATED_EXAMPLE_ID.fullmatch(example_id) is None:
            raise ValueError(f"{example_context}.id must be lowercase kebab case")
        if example_id in seen_ids:
            raise ValueError(f"{example_context}.id duplicates {example_id!r}")
        seen_ids.add(example_id)

        spans = tuple(
            _parse_related_span(span, f"{example_context}.spans[{span_index}]")
            for span_index, span in enumerate(
                expect_list(example["spans"], f"{example_context}.spans")
            )
        )
        if not spans:
            raise ValueError(f"{example_context}.spans must not be empty")
        if spans != tuple(sorted(spans, key=RelatedSpan.source_order_key)):
            raise ValueError(f"{example_context}.spans must use default source order")
        examples.append(RelatedSpanExample(example_id=example_id, spans=spans))
    if not examples:
        raise ValueError(f"{context} must not be empty")
    return tuple(examples)


def _parse_related_span_contract(
    value: YamlValue,
    context: str,
) -> RelatedSpanContract:
    """Parse the complete global RelatedSpan schema and examples."""
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"fields", "ordering", "examples"}, context)
    fields = expect_mapping(mapping["fields"], f"{context}.fields")
    expect_exact_keys(fields, {"required"}, f"{context}.fields")
    return RelatedSpanContract(
        required_fields=_parse_field_types(
            fields["required"],
            _RELATED_REQUIRED_FIELDS,
            f"{context}.fields.required",
        ),
        ordering=_parse_related_ordering(
            mapping["ordering"],
            f"{context}.ordering",
        ),
        examples=_parse_related_examples(
            mapping["examples"],
            f"{context}.examples",
        ),
    )


def load_contract_document(path: Path) -> DiagnosticContracts:
    """Load and validate the global and per-code diagnostic contracts."""
    raw = load_yaml(path)
    document = expect_mapping(raw, str(path))
    expect_exact_keys(
        document,
        {
            "version",
            "defaults",
            "source_span",
            "primary_span",
            "related_span",
            "codes",
        },
        str(path),
    )
    if document["version"] != "0.1":
        raise ValueError(f"{path}.version must be '0.1'")

    defaults = expect_mapping(document["defaults"], f"{path}.defaults")
    expect_exact_keys(defaults, {"help", "related"}, f"{path}.defaults")
    for field in ("help", "related"):
        if defaults[field] != "optional":
            raise ValueError(f"{path}.defaults.{field} must be optional")

    source_span = _parse_source_span_contract(
        document["source_span"],
        f"{path}.source_span",
    )
    primary_span = _parse_primary_span_contract(
        document["primary_span"],
        f"{path}.primary_span",
    )
    related_span = _parse_related_span_contract(
        document["related_span"],
        f"{path}.related_span",
    )
    raw_codes = expect_mapping(document["codes"], f"{path}.codes")
    contracts: dict[str, DiagnosticContract] = {}
    for code, raw_contract in raw_codes.items():
        context = f"{path}.codes.{code}"
        if not RE_CODE.fullmatch(code):
            raise ValueError(f"{context} is not an uppercase snake-case code")
        contract = expect_mapping(raw_contract, context)
        expect_exact_keys(contract, {"severity", "message", "data"}, context)
        severity = expect_str(contract["severity"], f"{context}.severity")
        if severity not in SEVERITIES:
            raise ValueError(f"{context}.severity must be one of {sorted(SEVERITIES)}")
        all_of, any_of, templates, contextual = parse_message(
            contract["message"], f"{context}.message"
        )
        required, required_if_emitted, conditional, exact_keys = parse_data(
            contract["data"], f"{context}.data"
        )
        contracts[code] = DiagnosticContract(
            severity=severity,
            message_all_of=all_of,
            message_any_of=any_of,
            message_templates=templates,
            message_contextual=contextual,
            data_required=required,
            data_required_if_emitted=required_if_emitted,
            data_conditional_required=conditional,
            data_exact_value_keys=exact_keys,
        )
    if not contracts:
        raise ValueError(f"{path}.codes must not be empty")
    return DiagnosticContracts(
        source_span=source_span,
        primary_span=primary_span,
        related_span=related_span,
        codes=contracts,
    )


def load_contracts(path: Path) -> dict[str, DiagnosticContract]:
    """Load the per-code projection after validating the whole contract document."""
    return load_contract_document(path).codes

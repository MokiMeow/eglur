"""Project visible Markdown into link destinations and heading anchors."""

import html
import re
import string
from collections.abc import Sequence
from collections.abc import Set as AbstractSet
from dataclasses import dataclass

from eglur_conformance import markdown_document
from eglur_conformance.documentation import model

_ATX_HEADING = re.compile(r" {0,3}#{1,6}(?:[ \t]+(?P<text>.*)|[ \t]*)$")
_REFERENCE_DEFINITION = re.compile(
    r" {0,3}\[(?P<label>[^]]+)]\s*:\s*(?P<destination>.*)$"
)
_REFERENCE_USE = re.compile(
    r"(?<!\\)(?P<image>!)?\[(?P<label>[^]]+)]\[(?P<reference>[^]]*)]"
)
_SHORTCUT_REFERENCE = re.compile(r"(?<!\\)(?P<image>!)?\[(?P<label>[^]]+)]")
_INLINE_LABEL = re.compile(r"(?<!\\)]\(")
_TRAILING_HEADING_MARKERS = re.compile(r"[ \t]+#+[ \t]*$")
_INLINE_LINK_TEXT = re.compile(r"!?\[([^]]+)]\([^)]*\)")
_HTML_TAG = re.compile(r"<[^>]+>")


@dataclass(frozen=True)
class _ReferenceDefinition:
    label: str
    line: int
    column: int
    destination: str


@dataclass(frozen=True)
class _ReferenceUse:
    label: str
    line: int
    column: int


def _mask_inline_code(line: str) -> str:
    masked = list(line)
    offset = 0
    while offset < len(line):
        code_start = line.find("`", offset)
        if code_start < 0:
            break
        run_length = 1
        while (
            code_start + run_length < len(line) and line[code_start + run_length] == "`"
        ):
            run_length += 1
        code_end = markdown_document.find_code_span_end(
            line,
            code_start + run_length,
            run_length,
        )
        if code_end is None:
            offset = code_start + run_length
            continue
        masked[code_start:code_end] = " " * (code_end - code_start)
        offset = code_end
    return "".join(masked)


def _markdown_unescape(value: str) -> str:
    unescaped: list[str] = []
    offset = 0
    while offset < len(value):
        if (
            value[offset] == "\\"
            and offset + 1 < len(value)
            and value[offset + 1] in string.punctuation
        ):
            offset += 1
        unescaped.append(value[offset])
        offset += 1
    return html.unescape("".join(unescaped))


def _find_outer_close(line: str, start: int) -> int | None:
    quote: str | None = None
    nested_parentheses = 0
    offset = start
    while offset < len(line):
        character = line[offset]
        if character == "\\" and offset + 1 < len(line):
            offset += 2
            continue
        if quote is not None:
            if character == quote:
                quote = None
            offset += 1
            continue
        if character in {'"', "'"}:
            quote = character
        elif character == "(":
            nested_parentheses += 1
        elif character == ")":
            if nested_parentheses == 0:
                return offset
            nested_parentheses -= 1
        offset += 1
    return None


def _parse_inline_destination(
    original: str,
    visible: str,
    start: int,
) -> tuple[str, int, int] | None:
    offset = start
    while offset < len(visible) and visible[offset].isspace():
        offset += 1
    destination_start = offset

    if offset < len(visible) and visible[offset] == "<":
        destination_start = offset + 1
        offset += 1
        while offset < len(visible):
            if visible[offset] == "\\" and offset + 1 < len(visible):
                offset += 2
                continue
            if visible[offset] == ">":
                destination_end = offset
                close = _find_outer_close(visible, offset + 1)
                if close is None:
                    return None
                return (
                    _markdown_unescape(original[destination_start:destination_end]),
                    destination_start,
                    close + 1,
                )
            offset += 1
        return None

    nested_parentheses = 0
    while offset < len(visible):
        character = visible[offset]
        if character == "\\" and offset + 1 < len(visible):
            offset += 2
            continue
        if character == "(":
            nested_parentheses += 1
        elif character == ")":
            if nested_parentheses == 0:
                return (
                    _markdown_unescape(original[destination_start:offset]),
                    destination_start,
                    offset + 1,
                )
            nested_parentheses -= 1
        elif character.isspace() and nested_parentheses == 0:
            close = _find_outer_close(visible, offset)
            if close is None:
                return None
            return (
                _markdown_unescape(original[destination_start:offset]),
                destination_start,
                close + 1,
            )
        offset += 1
    return None


def _inline_destinations(
    line_number: int,
    line: str,
) -> tuple[model.LocatedDestination, ...]:
    visible = _mask_inline_code(line)
    destinations: list[model.LocatedDestination] = []
    offset = 0
    while matched := _INLINE_LABEL.search(visible, offset):
        label_start = visible.rfind("[", 0, matched.start())
        if label_start < 0 or (label_start > 0 and visible[label_start - 1] == "\\"):
            offset = matched.end()
            continue
        parsed = _parse_inline_destination(
            line,
            visible,
            matched.end(),
        )
        if parsed is None:
            offset = matched.end()
            continue
        destination, destination_start, link_end = parsed
        destinations.append(
            model.LocatedDestination(
                line=line_number,
                column=destination_start + 1,
                destination=destination,
            )
        )
        offset = link_end
    return tuple(destinations)


def _normalize_reference_label(label: str) -> str:
    return " ".join(label.split()).casefold()


def _parse_reference_destination(value: str) -> tuple[str, int] | None:
    leading = len(value) - len(value.lstrip())
    stripped = value.lstrip()
    if not stripped:
        return None
    if stripped.startswith("<"):
        end = stripped.find(">", 1)
        if end < 0:
            return None
        return _markdown_unescape(stripped[1:end]), leading + 1
    matched = re.match(r"[^\s]+", stripped)
    if matched is None:
        return None
    return _markdown_unescape(matched.group(0)), leading


def _reference_definitions(
    visible_lines: Sequence[tuple[int, str]],
) -> dict[str, _ReferenceDefinition]:
    definitions: dict[str, _ReferenceDefinition] = {}
    for line_number, line in visible_lines:
        visible = _mask_inline_code(line)
        matched = _REFERENCE_DEFINITION.fullmatch(visible)
        if matched is None:
            continue
        label = _normalize_reference_label(matched.group("label"))
        parsed = _parse_reference_destination(matched.group("destination"))
        if not label or parsed is None or label in definitions:
            continue
        destination, offset = parsed
        destination_column = matched.start("destination") + offset + 1
        definitions[label] = _ReferenceDefinition(
            label=label,
            line=line_number,
            column=destination_column,
            destination=destination,
        )
    return definitions


def _reference_uses(
    visible_lines: Sequence[tuple[int, str]],
    known_labels: AbstractSet[str],
) -> tuple[_ReferenceUse, ...]:
    uses: list[_ReferenceUse] = []
    for line_number, line in visible_lines:
        visible = _mask_inline_code(line)
        definition = _REFERENCE_DEFINITION.fullmatch(visible)
        definition_span = None if definition is None else definition.span()
        occupied: list[tuple[int, int]] = []
        for matched in _REFERENCE_USE.finditer(visible):
            label = matched.group("reference") or matched.group("label")
            uses.append(
                _ReferenceUse(
                    label=_normalize_reference_label(label),
                    line=line_number,
                    column=matched.start() + 1,
                )
            )
            occupied.append(matched.span())

        for matched in _SHORTCUT_REFERENCE.finditer(visible):
            if definition_span is not None and matched.start() >= definition_span[0]:
                continue
            if any(start <= matched.start() < end for start, end in occupied):
                continue
            if matched.end() < len(visible) and visible[matched.end()] in "([":
                continue
            label = _normalize_reference_label(matched.group("label"))
            if label not in known_labels:
                continue
            uses.append(
                _ReferenceUse(
                    label=label,
                    line=line_number,
                    column=matched.start() + 1,
                )
            )
    return tuple(uses)


def _rendered_heading_text(text: str) -> str:
    rendered = _TRAILING_HEADING_MARKERS.sub("", text)
    rendered = _INLINE_LINK_TEXT.sub(r"\1", rendered)
    rendered = _HTML_TAG.sub("", rendered)
    rendered = rendered.replace("`", "").replace("*", "").replace("~", "")
    return html.unescape(rendered)


def _heading_slug(text: str) -> str:
    normalized: list[str] = []
    for character in _rendered_heading_text(text).strip().lower():
        if character.isalnum() or character in {"-", "_"}:
            normalized.append(character)
        elif character.isspace():
            normalized.append("-")
    return "".join(normalized)


def _heading_anchors(
    visible_lines: Sequence[tuple[int, str]],
) -> frozenset[str]:
    counts: dict[str, int] = {}
    anchors: set[str] = set()
    for _, line in visible_lines:
        matched = _ATX_HEADING.fullmatch(line)
        if matched is None:
            continue
        slug = _heading_slug(matched.group("text") or "")
        duplicate_count = counts.get(slug, 0)
        anchor = slug if duplicate_count == 0 else f"{slug}-{duplicate_count}"
        counts[slug] = duplicate_count + 1
        anchors.add(anchor)
    return frozenset(anchors)


def parse_document(source: model.MarkdownSource) -> model.ParsedMarkdownLinks:
    """Project one Markdown source into local-link syntax facts.

    Parameters
    ----------
    source : MarkdownSource
        Canonical tracked source and repository identity.

    Returns
    -------
    ParsedMarkdownLinks
        Heading anchors, link destinations, and undefined references.

    Raises
    ------
    ValueError
        If the source has an unterminated hidden region.
    """
    visible_lines = tuple(
        markdown_document.visible_markdown_lines_from_text(
            source.text.removeprefix("\ufeff"),
            source.path.as_posix(),
            preserve_columns=True,
        )
    )
    definitions = _reference_definitions(visible_lines)
    uses = _reference_uses(visible_lines, definitions.keys())
    used_definitions: set[str] = set()
    undefined: list[model.UndefinedReference] = []
    for use in uses:
        if use.label not in definitions:
            undefined.append(
                model.UndefinedReference(
                    label=use.label,
                    line=use.line,
                    column=use.column,
                )
            )
            continue
        used_definitions.add(use.label)

    destinations = [
        destination
        for line_number, line in visible_lines
        for destination in _inline_destinations(line_number, line)
    ]
    destinations.extend(
        model.LocatedDestination(
            line=definition.line,
            column=definition.column,
            destination=definition.destination,
        )
        for label, definition in definitions.items()
        if label in used_definitions
    )
    return model.ParsedMarkdownLinks(
        path=source.path,
        anchors=_heading_anchors(visible_lines),
        destinations=tuple(destinations),
        undefined_references=tuple(undefined),
    )

"""Read visible metadata and fenced examples from Markdown documents."""

import re
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

RE_FENCE_OPEN = re.compile(r" {0,3}(?P<run>`{3,}|~{3,})(?P<tail>.*)")
_RE_LIST_ITEM = re.compile(r"(?P<indent> *)(?:[*+-]|\d{1,9}[.)])(?P<separator>[ \t]+)")


@dataclass(frozen=True)
class FencedCodeBlock:
    """One source-located Markdown fenced code block."""

    opening_line: int
    info: str
    code: str


@dataclass(frozen=True)
class _ListItemContext:
    marker_indent: int
    content_indent: int


def _leading_spaces(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _update_list_context(
    line: str,
    contexts: list[_ListItemContext],
) -> bool:
    """Update active Markdown list nesting and report a list-marker line."""
    if not line.strip():
        return False

    matched = _RE_LIST_ITEM.match(line)
    if matched is not None:
        marker_indent = len(matched.group("indent"))
        while contexts and marker_indent < contexts[-1].marker_indent:
            contexts.pop()

        parent = contexts[-1] if contexts else None
        starts_top_level_item = parent is None and marker_indent <= 3
        starts_nested_item = (
            parent is not None
            and marker_indent > parent.marker_indent
            and marker_indent < parent.content_indent + 4
        )
        replaces_sibling = parent is not None and marker_indent == parent.marker_indent
        if starts_top_level_item or starts_nested_item or replaces_sibling:
            if replaces_sibling:
                contexts.pop()
            contexts.append(
                _ListItemContext(
                    marker_indent=marker_indent,
                    content_indent=matched.end(),
                )
            )
            return True

    indentation = _leading_spaces(line)
    while contexts and indentation <= contexts[-1].marker_indent:
        contexts.pop()
    return False


def _container_indent(
    line: str,
    contexts: list[_ListItemContext],
) -> int:
    if not contexts:
        return 0
    context = contexts[-1]
    indentation = _leading_spaces(line)
    if indentation <= context.marker_indent:
        return 0
    return min(indentation, context.content_indent)


def _without_container_indent(line: str, container_indent: int) -> str:
    if _leading_spaces(line) < container_indent:
        return line
    return line[container_indent:]


def _is_indented_code(
    line: str,
    contexts: list[_ListItemContext],
    *,
    is_list_item: bool,
) -> bool:
    if is_list_item:
        return False
    if line.startswith("\t"):
        return True

    indentation = _leading_spaces(line)
    if not contexts:
        return indentation >= 4
    return indentation >= contexts[-1].content_indent + 4


def find_code_span_end(line: str, start: int, run_length: int) -> int | None:
    """Return the end of an exact-length backtick span on the same line."""
    delimiter = "`" * run_length
    candidate = line.find(delimiter, start)
    while candidate >= 0:
        before_is_backtick = candidate > 0 and line[candidate - 1] == "`"
        after = candidate + run_length
        after_is_backtick = after < len(line) and line[after] == "`"
        if not before_is_backtick and not after_is_backtick:
            return after
        candidate = line.find(delimiter, candidate + 1)
    return None


def strip_html_comments(
    line: str,
    in_comment: bool,
    *,
    preserve_columns: bool = False,
) -> tuple[str, bool]:
    """Hide HTML comments while preserving visible text.

    Parameters
    ----------
    line : str
        One Markdown source line.
    in_comment : bool
        Whether a comment began on an earlier line.
    preserve_columns : bool, default=False
        Replace comment text with spaces instead of removing it.

    Returns
    -------
    tuple[str, bool]
        Visible text and the comment state after this line.
    """
    visible: list[str] = []
    offset = 0

    while offset < len(line):
        if in_comment:
            comment_end = line.find("-->", offset)
            if comment_end < 0:
                if preserve_columns:
                    visible.append(" " * (len(line) - offset))
                return "".join(visible), True
            if preserve_columns:
                visible.append(" " * (comment_end + 3 - offset))
            offset = comment_end + 3
            in_comment = False
            continue

        comment_start = line.find("<!--", offset)
        code_start = line.find("`", offset)
        if code_start >= 0 and (comment_start < 0 or code_start < comment_start):
            run_length = 1
            while (
                code_start + run_length < len(line)
                and line[code_start + run_length] == "`"
            ):
                run_length += 1
            code_end = find_code_span_end(
                line,
                code_start + run_length,
                run_length,
            )
            if code_end is None:
                visible.append(line[offset : code_start + run_length])
                offset = code_start + run_length
            else:
                visible.append(line[offset:code_end])
                offset = code_end
            continue
        if comment_start < 0:
            visible.append(line[offset:])
            break
        visible.append(line[offset:comment_start])
        if preserve_columns:
            visible.append(" " * 4)
        offset = comment_start + 4
        in_comment = True

    return "".join(visible), in_comment


def is_fence_close(line: str, character: str, minimum_length: int) -> bool:
    """Return whether one line closes the active Markdown fence."""
    stripped = line.lstrip(" ")
    indentation = len(line) - len(stripped)
    if indentation > 3 or not stripped.startswith(character * minimum_length):
        return False
    run_length = len(stripped) - len(stripped.lstrip(character))
    return run_length >= minimum_length and stripped[run_length:].strip() == ""


def fenced_code_blocks(path: Path) -> Iterator[FencedCodeBlock]:
    """Yield visible fenced code blocks with their info strings and source lines."""
    fence_character: str | None = None
    fence_length = 0
    opening_line = 0
    info = ""
    code_lines: list[str] = []
    in_html_comment = False

    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(),
        1,
    ):
        if fence_character is not None:
            if is_fence_close(line, fence_character, fence_length):
                code = "\n".join(code_lines)
                if code_lines:
                    code += "\n"
                yield FencedCodeBlock(
                    opening_line=opening_line,
                    info=info,
                    code=code,
                )
                fence_character = None
                fence_length = 0
                opening_line = 0
                info = ""
                code_lines = []
            else:
                code_lines.append(line)
            continue

        matched_fence = None
        if not in_html_comment:
            matched_fence = RE_FENCE_OPEN.fullmatch(line)

        if matched_fence is None:
            visible, in_html_comment = strip_html_comments(line, in_html_comment)
            matched_fence = RE_FENCE_OPEN.fullmatch(visible)

        if matched_fence is not None:
            run = matched_fence.group("run")
            fence_character = run[0]
            fence_length = len(run)
            opening_line = line_number
            info = matched_fence.group("tail").strip()
            code_lines = []
            in_html_comment = False

    if fence_character is not None:
        raise ValueError(f"{path}: unterminated Markdown code fence")
    if in_html_comment:
        raise ValueError(f"{path}: unterminated HTML comment")


def visible_markdown_lines_from_text(
    source: str,
    source_name: str,
    *,
    preserve_columns: bool = False,
) -> Iterator[tuple[int, str]]:
    """Yield visible, source-located lines from Markdown text.

    Parameters
    ----------
    source : str
        Markdown source text.
    source_name : str
        Source identity used in malformed-document errors.
    preserve_columns : bool, default=False
        Replace hidden HTML comment text with spaces instead of removing it.

    Yields
    ------
    tuple[int, str]
        One-based line number and text outside comments and code blocks.

    Raises
    ------
    ValueError
        If a fenced code block or HTML comment is unterminated.
    """
    fence_character: str | None = None
    fence_length = 0
    fence_container_indent = 0
    in_html_comment = False
    list_contexts: list[_ListItemContext] = []

    for line_number, line in enumerate(source.splitlines(), 1):
        if fence_character is not None:
            fence_line = _without_container_indent(line, fence_container_indent)
            if is_fence_close(fence_line, fence_character, fence_length):
                fence_character = None
                fence_length = 0
                fence_container_indent = 0
            continue

        was_in_html_comment = in_html_comment
        logical_visible, next_comment_state = strip_html_comments(
            line,
            in_html_comment,
        )
        if preserve_columns:
            visible, in_html_comment = strip_html_comments(
                line,
                in_html_comment,
                preserve_columns=True,
            )
        else:
            visible = logical_visible
            in_html_comment = next_comment_state
        if not logical_visible.strip() and (
            in_html_comment or was_in_html_comment or "<!--" in line
        ):
            continue

        is_list_item = _update_list_context(logical_visible, list_contexts)
        container_indent = _container_indent(logical_visible, list_contexts)
        logical_container_line = _without_container_indent(
            logical_visible,
            container_indent,
        )
        source_container_line = _without_container_indent(line, container_indent)
        matched_fence = None
        if not was_in_html_comment:
            matched_fence = RE_FENCE_OPEN.fullmatch(source_container_line)
        if matched_fence is None:
            matched_fence = RE_FENCE_OPEN.fullmatch(logical_container_line)
        if matched_fence is not None:
            run = matched_fence.group("run")
            fence_character = run[0]
            fence_length = len(run)
            fence_container_indent = container_indent
            in_html_comment = False
            continue

        if _is_indented_code(
            logical_visible,
            list_contexts,
            is_list_item=is_list_item,
        ):
            continue
        yield line_number, visible

    if fence_character is not None:
        raise ValueError(f"{source_name}: unterminated Markdown code fence")
    if in_html_comment:
        raise ValueError(f"{source_name}: unterminated HTML comment")


def visible_markdown_lines(path: Path) -> Iterator[tuple[int, str]]:
    """Yield source-located lines outside comments and Markdown code blocks."""
    yield from visible_markdown_lines_from_text(
        path.read_text(encoding="utf-8"),
        str(path),
    )

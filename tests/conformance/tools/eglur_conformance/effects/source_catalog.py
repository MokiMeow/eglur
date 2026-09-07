"""Extract callable identities from normative stdlib catalog blocks."""

import re
from pathlib import Path

from eglur_conformance.effects.model import (
    AliasKey,
    CallableKey,
    CallbackContract,
    CallbackKind,
    SourceAlias,
    SourceCallable,
    SourceCatalog,
)
from eglur_conformance.markdown_document import (
    FencedCodeBlock,
    fenced_code_blocks,
    visible_markdown_lines,
)

_RE_BASELINE_HEADING = re.compile(r"^###\s+Baseline Modules \(v0\.1\)\s*$")
_RE_SECTION_HEADING = re.compile(r"^#{1,3}\s+")
_RE_MODULE_LINK = re.compile(r"\[[^\]]*`std::[^`]+`[^\]]*\]\((stdlib/[^)]+\.md)\)")
_RE_HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
_RE_MODULE = re.compile(r"`(std::[A-Za-z0-9_:]+)`")
_RE_TRAIT_OPEN = re.compile(
    r"^\s*(?:closed\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)[^{}]*\{\s*$"
)
_RE_CALLABLE_START = re.compile(
    r"^\s*(?:(?:optional|overload)\s+)*fn\s+"
    r"|^\s*(?:optional\s+)?callable\s+"
    r"|^\s*transform\s+"
)
_RE_CALLBACK = re.compile(
    r"\b([A-Za-z_][A-Za-z0-9_]*):\s*"
    r"callback\[(pure|effectful|scoped-resource|task-thunk)\]"
)
_RE_REEXPORT_FN = re.compile(
    r"^\s*reexport\s+fn\s+([A-Za-z_][A-Za-z0-9_]*!?)\s*=\s*"
    r"(std::[A-Za-z0-9_:!]+)\s*$"
)
_RE_REQUIRES = re.compile(r"^\s*requires\b")
_RE_ANY_DECLARATION_START = re.compile(
    r"^\s*(?:optional\s+)?(?:"
    r"opaque\s+type|alias\s+type|sum\s+type|union\s+type|meta\s+type|"
    r"reexport\s+(?:type|trait|fn)|(?:closed\s+)?trait|impl|value|schema|transform|"
    r"(?:overload\s+)?fn|callable"
    r")\b"
)
_RE_FN_TOKEN = re.compile(r"\bfn\s+")
_RE_CALLABLE_TOKEN = re.compile(r"\bcallable\s+")
_RE_TRANSFORM_TOKEN = re.compile(r"\btransform\s+")


def _baseline_module_sources(index_path: Path, repo_root: Path) -> tuple[str, ...]:
    """Extract baseline module-document paths from the stdlib section index.

    Parameters
    ----------
    index_path
        Path to the normative standard-library section index.
    repo_root
        Repository root used to normalize linked paths.

    Returns
    -------
    tuple[str, ...]
        Ordered repository-relative baseline module paths.
    """
    in_baseline = False
    sources: list[str] = []
    for _, line in visible_markdown_lines(index_path):
        stripped = line.strip()
        if _RE_BASELINE_HEADING.fullmatch(stripped):
            if in_baseline:
                raise ValueError(f"{index_path}: duplicate baseline module section")
            in_baseline = True
            continue
        if in_baseline and _RE_SECTION_HEADING.match(stripped):
            break
        if not in_baseline:
            continue
        matched_link = _RE_MODULE_LINK.search(stripped)
        if matched_link is None:
            continue
        source = (index_path.parent / matched_link.group(1)).resolve()
        try:
            relative = source.relative_to(repo_root.resolve()).as_posix()
        except ValueError as error:
            raise ValueError(
                f"{index_path}: baseline module path escapes repository: {source}"
            ) from error
        sources.append(relative)

    if not in_baseline:
        raise ValueError(f"{index_path}: missing baseline module section")
    if not sources:
        raise ValueError(f"{index_path}: baseline module section has no module links")
    if len(sources) != len(set(sources)):
        raise ValueError(f"{index_path}: baseline module paths must be unique")
    return tuple(sources)


def _module_for_block(path: Path, block: FencedCodeBlock) -> str:
    heading_stack: list[tuple[int, str]] = []
    for line_number, line in visible_markdown_lines(path):
        if line_number >= block.opening_line:
            break
        matched_heading = _RE_HEADING.fullmatch(line.strip())
        if matched_heading is None:
            continue
        level = len(matched_heading.group(1))
        heading_stack = [heading for heading in heading_stack if heading[0] < level]
        heading_stack.append((level, matched_heading.group(2)))

    for _, heading in reversed(heading_stack):
        modules = _RE_MODULE.findall(heading)
        if len(modules) > 1:
            raise ValueError(
                f"{path}:{block.opening_line}: selected heading names multiple modules"
            )
        if modules:
            return modules[0]
    raise ValueError(f"{path}:{block.opening_line}: no catalog module binding")


def _normalize_declaration(lines: list[str]) -> str:
    return re.sub(r"\s+", " ", " ".join(line.strip() for line in lines)).strip()


def _declaration_complete(lines: list[str]) -> bool:
    text = " ".join(lines)
    return (
        "->" in text
        and text.count("(") == text.count(")")
        and text.count("[") == text.count("]")
    )


def _is_bang_declaration(declaration: str) -> bool:
    head = declaration.split("(", 1)[0]
    return "!" in head


def _extract_block(
    path: Path,
    relative_path: str,
    block: FencedCodeBlock,
) -> tuple[list[SourceCallable], list[SourceAlias]]:
    module = _module_for_block(path, block)
    callables: list[SourceCallable] = []
    aliases: list[SourceAlias] = []
    lines = block.code.splitlines()
    trait = ""
    index = 0

    while index < len(lines):
        line = lines[index]
        matched_trait = _RE_TRAIT_OPEN.fullmatch(line)
        if matched_trait is not None:
            trait = matched_trait.group(1)
            index += 1
            continue
        if trait and line.strip() == "}":
            trait = ""
            index += 1
            continue

        matched_alias = _RE_REEXPORT_FN.fullmatch(line)
        if matched_alias is not None:
            aliases.append(
                SourceAlias(
                    key=AliasKey(
                        module=module,
                        name=matched_alias.group(1),
                        target=matched_alias.group(2),
                    ),
                    source_path=relative_path,
                    source_line=block.opening_line + index + 1,
                )
            )
            index += 1
            continue

        if _RE_CALLABLE_START.match(line) is None:
            index += 1
            continue

        declaration_lines = [line]
        source_line = block.opening_line + index + 1
        index += 1
        while index < len(lines) and not _declaration_complete(declaration_lines):
            declaration_lines.append(lines[index])
            index += 1
        if not _declaration_complete(declaration_lines):
            raise ValueError(f"{relative_path}:{source_line}: incomplete callable")
        if index < len(lines) and _RE_REQUIRES.match(lines[index]):
            while index < len(lines):
                continuation = lines[index]
                if continuation.strip() == "}" or _RE_ANY_DECLARATION_START.match(
                    continuation
                ):
                    break
                if not continuation.strip() and declaration_lines[-1].strip():
                    break
                declaration_lines.append(continuation)
                index += 1

        declaration = _normalize_declaration(declaration_lines)
        callbacks = tuple(
            CallbackContract(parameter=parameter, kind=CallbackKind(kind))
            for parameter, kind in _RE_CALLBACK.findall(declaration)
        )
        if len({callback.parameter for callback in callbacks}) != len(callbacks):
            raise ValueError(
                f"{relative_path}:{source_line}: duplicate callback parameter"
            )
        callables.append(
            SourceCallable(
                key=CallableKey(
                    module=module,
                    trait=trait,
                    declaration=declaration,
                ),
                callbacks=callbacks,
                bang=_is_bang_declaration(declaration),
                source_path=relative_path,
                source_line=source_line,
            )
        )

    parsed_fn_count = len(aliases) + sum(
        bool(re.match(r"^(?:(?:optional|overload)\s+)*fn\s+", item.key.declaration))
        for item in callables
    )
    parsed_callable_count = sum(
        item.key.declaration.startswith(("callable ", "optional callable "))
        for item in callables
    )
    parsed_transform_count = sum(
        item.key.declaration.startswith("transform ") for item in callables
    )
    observed_counts = (
        len(_RE_FN_TOKEN.findall(block.code)),
        len(_RE_CALLABLE_TOKEN.findall(block.code)),
        len(_RE_TRANSFORM_TOKEN.findall(block.code)),
    )
    parsed_counts = (
        parsed_fn_count,
        parsed_callable_count,
        parsed_transform_count,
    )
    if parsed_counts != observed_counts:
        raise ValueError(
            f"{relative_path}:{block.opening_line}: unparsed callable declaration; "
            f"parsed fn/callable/transform={parsed_counts}, observed={observed_counts}"
        )

    return callables, aliases


def extract_source_catalog(
    repo_root: Path,
    baseline_index: str,
) -> SourceCatalog:
    """Extract all callable records from indexed baseline module documents.

    Parameters
    ----------
    repo_root
        Repository root containing the specification package.
    baseline_index
        Repository-relative standard-library section index path.

    Returns
    -------
    SourceCatalog
        Normalized callable declarations, callback positions, and aliases.
    """
    index_path = repo_root / baseline_index
    sources = _baseline_module_sources(index_path, repo_root)
    callables: list[SourceCallable] = []
    aliases: list[SourceAlias] = []

    for relative_path in sources:
        path = repo_root / relative_path
        for block in fenced_code_blocks(path):
            if block.info != "eglur-catalog":
                continue
            block_callables, block_aliases = _extract_block(
                path,
                relative_path,
                block,
            )
            callables.extend(block_callables)
            aliases.extend(block_aliases)

    callable_keys = [callable_record.key for callable_record in callables]
    if len(callable_keys) != len(set(callable_keys)):
        duplicates = sorted(
            key.render() for key in set(callable_keys) if callable_keys.count(key) > 1
        )
        raise ValueError(f"duplicate stdlib callable declarations: {duplicates}")
    alias_keys = [alias.key for alias in aliases]
    if len(alias_keys) != len(set(alias_keys)):
        raise ValueError("duplicate stdlib callable re-exports")

    return SourceCatalog(
        sources=sources,
        callables=tuple(callables),
        aliases=tuple(aliases),
    )

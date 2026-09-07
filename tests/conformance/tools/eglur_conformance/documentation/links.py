"""Validate repository-local Markdown destinations against tracked paths."""

from collections.abc import Sequence
from collections.abc import Set as AbstractSet
from pathlib import PurePosixPath
from urllib.parse import unquote, urlsplit

from eglur_conformance.documentation import model, syntax


def _resolve_local_path(
    source_path: PurePosixPath,
    destination_path: str,
) -> tuple[PurePosixPath | None, str | None]:
    decoded = unquote(destination_path)
    if decoded.startswith("/"):
        return None, "local target escapes repository root"
    if "\\" in decoded or "\x00" in decoded:
        return None, "local target is not a canonical POSIX path"

    components = (
        []
        if source_path.parent == PurePosixPath(".")
        else list(source_path.parent.parts)
    )
    for component in decoded.split("/"):
        if component in {"", "."}:
            continue
        if component == "..":
            if not components:
                return None, "local target escapes repository root"
            components.pop()
            continue
        components.append(component)
    if not components:
        return PurePosixPath("."), None
    return PurePosixPath(*components), None


def _destination_violation(
    source_path: PurePosixPath,
    located: model.LocatedDestination,
    targets: AbstractSet[PurePosixPath],
    anchors: dict[PurePosixPath, frozenset[str]],
) -> model.LinkViolation | None:
    destination = located.destination
    try:
        parsed = urlsplit(destination)
    except ValueError:
        return model.LinkViolation(
            source_path,
            located.line,
            located.column,
            destination,
            "destination is not a valid URI reference",
        )
    if parsed.scheme or parsed.netloc:
        return None

    target, reason = _resolve_local_path(source_path, parsed.path)
    if reason is not None or target is None:
        return model.LinkViolation(
            source_path,
            located.line,
            located.column,
            destination,
            reason or "local target is invalid",
        )
    if parsed.path == "":
        target = source_path
    if target not in targets:
        return model.LinkViolation(
            source_path,
            located.line,
            located.column,
            destination,
            "local target is not tracked and present",
        )

    if not parsed.fragment:
        return None
    if target.suffix != ".md":
        return model.LinkViolation(
            source_path,
            located.line,
            located.column,
            destination,
            "fragment requires a Markdown target",
        )
    fragment = unquote(parsed.fragment)
    if fragment not in anchors.get(target, frozenset()):
        return model.LinkViolation(
            source_path,
            located.line,
            located.column,
            destination,
            "fragment does not exist in Markdown target",
        )
    return None


def find_violations(
    sources: Sequence[model.MarkdownSource],
    targets: AbstractSet[PurePosixPath],
) -> list[model.LinkViolation]:
    """Return invalid local links in canonical source order.

    Parameters
    ----------
    sources : collections.abc.Sequence[MarkdownSource]
        Tracked Markdown worktree sources.
    targets : collections.abc.Set[pathlib.PurePosixPath]
        Exact tracked and present files plus their repository directories.

    Returns
    -------
    list[LinkViolation]
        Deterministically ordered source-located violations.

    Raises
    ------
    ValueError
        If a Markdown source has an unterminated hidden region.
    """
    documents = tuple(
        syntax.parse_document(source)
        for source in sorted(sources, key=lambda item: item.path.as_posix())
    )
    anchors = {document.path: document.anchors for document in documents}
    violations: list[model.LinkViolation] = []

    for document in documents:
        violations.extend(
            model.LinkViolation(
                path=document.path,
                line=reference.line,
                column=reference.column,
                destination=reference.label,
                reason="reference is undefined",
            )
            for reference in document.undefined_references
        )
        for located in document.destinations:
            violation = _destination_violation(
                document.path,
                located,
                targets,
                anchors,
            )
            if violation is not None:
                violations.append(violation)

    return sorted(
        violations,
        key=lambda item: (
            item.path.as_posix(),
            item.line,
            item.column,
            item.destination,
            item.reason,
        ),
    )

"""Load exact tracked Markdown sources and repository link targets."""

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from eglur_conformance.documentation import model


@dataclass(frozen=True)
class TrackedDocumentation:
    """One immutable projection of the tracked documentation worktree.

    Parameters
    ----------
    sources : tuple[MarkdownSource, ...]
        Tracked Markdown files in canonical path order.
    targets : frozenset[pathlib.PurePosixPath]
        Present tracked files and their repository directories.
    """

    sources: tuple[model.MarkdownSource, ...]
    targets: frozenset[PurePosixPath]


def _require_worktree_root(repo_root: Path) -> None:
    completed = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--show-toplevel"],
        cwd=repo_root,
        check=False,
        capture_output=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.decode("utf-8", errors="replace").strip()
        raise ValueError(f"cannot locate Git worktree root: {detail}")
    try:
        discovered = Path(completed.stdout.decode("utf-8").strip()).resolve(strict=True)
    except UnicodeDecodeError as error:
        raise ValueError("Git worktree root is not valid UTF-8") from error
    if discovered != repo_root:
        raise ValueError(
            f"repository root is not the Git worktree root: {repo_root} "
            f"(expected {discovered})"
        )


def _tracked_paths(repo_root: Path) -> tuple[PurePosixPath, ...]:
    completed = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=repo_root,
        check=False,
        capture_output=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.decode("utf-8", errors="replace").strip()
        raise ValueError(f"cannot enumerate tracked repository paths: {detail}")
    try:
        decoded = completed.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError("tracked repository path is not valid UTF-8") from error
    return tuple(
        sorted(
            (PurePosixPath(item) for item in decoded.split("\0") if item),
            key=PurePosixPath.as_posix,
        )
    )


def _has_symlink_component(repo_root: Path, relative_path: PurePosixPath) -> bool:
    candidate = repo_root
    for component in relative_path.parts:
        candidate /= component
        if candidate.is_symlink():
            return True
    return False


def _present_targets(
    repo_root: Path,
    tracked_paths: tuple[PurePosixPath, ...],
) -> frozenset[PurePosixPath]:
    targets: set[PurePosixPath] = {PurePosixPath(".")}
    for relative_path in tracked_paths:
        worktree_path = repo_root.joinpath(*relative_path.parts)
        if not os.path.lexists(worktree_path):
            continue
        targets.add(relative_path)
        parent = relative_path.parent
        while parent != PurePosixPath("."):
            targets.add(parent)
            parent = parent.parent
    return frozenset(targets)


def load_tracked_documentation(repo_root: Path) -> TrackedDocumentation:
    """Load tracked Markdown and exact present target identities.

    Parameters
    ----------
    repo_root : pathlib.Path
        Git worktree root.

    Returns
    -------
    TrackedDocumentation
        Canonical source and target projection for pure link validation.

    Raises
    ------
    OSError
        If the root or a tracked source cannot be read.
    ValueError
        If Git discovery or a tracked Markdown source violates the boundary.
    """
    resolved_root = repo_root.resolve(strict=True)
    if not resolved_root.is_dir():
        raise ValueError(f"repository root is not a directory: {repo_root}")
    _require_worktree_root(resolved_root)
    tracked_paths = _tracked_paths(resolved_root)
    targets = _present_targets(resolved_root, tracked_paths)
    sources: list[model.MarkdownSource] = []

    for relative_path in tracked_paths:
        if relative_path.suffix != ".md":
            continue
        worktree_path = resolved_root.joinpath(*relative_path.parts)
        try:
            if relative_path not in targets:
                raise ValueError("source is absent from the worktree")
            if _has_symlink_component(resolved_root, relative_path):
                raise ValueError("source path contains a symbolic link")
            if not worktree_path.is_file():
                raise ValueError("source is not a regular file")
            text = worktree_path.read_text(encoding="utf-8")
        except (OSError, UnicodeError, ValueError) as error:
            raise ValueError(
                "cannot load tracked Markdown source "
                f"{relative_path.as_posix()}: {error}"
            ) from error
        sources.append(model.MarkdownSource(path=relative_path, text=text))

    return TrackedDocumentation(sources=tuple(sources), targets=targets)

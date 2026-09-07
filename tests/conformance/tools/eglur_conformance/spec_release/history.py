"""Interpret Git history as specification-release lifecycle evidence."""

from dataclasses import dataclass
from pathlib import PurePosixPath
from typing import cast

import yaml

from eglur_conformance import release_identity
from eglur_conformance.spec_release import (
    git_repository,
    release_state,
    repository,
)
from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue, load_yaml_text


@dataclass(frozen=True)
class _StateInspection:
    """Decoded release state and validation failures for one commit."""

    state: release_state.ArtifactReleaseState | None
    problems: tuple[ValidationProblem, ...]


@dataclass(frozen=True)
class _HistoricalArtifact:
    """One carrier version reachable through the configured history root."""

    name: str
    query: git_repository.BlobQuery


def _expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def _expect_str(value: YamlValue, context: str) -> str:
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if value == "":
        raise ValueError(f"{context} must be non-empty")
    return value


def _carrier_queries(commit: str) -> tuple[git_repository.BlobQuery, ...]:
    return tuple(
        git_repository.BlobQuery(
            commit=commit,
            path=PurePosixPath(path.as_posix()),
        )
        for path in repository.RELEASE_STATE_PATHS.values()
    )


def _decode_state(
    commit: str,
    blobs: dict[git_repository.BlobQuery, str | None],
) -> _StateInspection:
    history_roots: dict[str, str] = {}
    releases: dict[str, release_identity.ReleaseLabel] = {}
    statuses: dict[str, str] = {}
    legacy_artifacts: list[str] = []
    problems: list[ValidationProblem] = []

    for (name, path), query in zip(
        repository.RELEASE_STATE_PATHS.items(),
        _carrier_queries(commit),
        strict=True,
    ):
        text = blobs[query]
        if text is None:
            legacy_artifacts.append(name)
            continue
        try:
            root = _expect_mapping(
                load_yaml_text(text),
                f"{commit}:{path}",
            )
            raw_history_root = root.get("release_history_root")
            raw_release = root.get("release")
            if raw_history_root is None and raw_release is None:
                legacy_artifacts.append(name)
                continue
            if raw_history_root is None or raw_release is None:
                raise ValueError(
                    f"{commit}:{path} must carry release_history_root "
                    "and release together"
                )
            history_root = _expect_str(
                raw_history_root,
                f"{commit}:{path}.release_history_root",
            )
            history_roots[name] = release_identity.validate_history_root(
                history_root,
                f"{commit}:{path}.release_history_root",
            )
            release = _expect_str(raw_release, f"{commit}:{path}.release")
            releases[name] = release_identity.parse_release_label(
                release,
                f"{commit}:{path}.release",
            )
            status = _expect_str(root.get("status"), f"{commit}:{path}.status")
            if status not in suite_schema.ALLOWED_ARTIFACT_STATUSES:
                raise ValueError(
                    f"{commit}:{path}.status must be one of "
                    f"{sorted(suite_schema.ALLOWED_ARTIFACT_STATUSES)}"
                )
            statuses[name] = status
        except (TypeError, ValueError, yaml.YAMLError) as error:
            problems.append(ValidationProblem(str(error)))

    if problems:
        return _StateInspection(state=None, problems=tuple(problems))
    if not releases:
        return _StateInspection(state=None, problems=())
    if legacy_artifacts:
        return _StateInspection(
            state=None,
            problems=(
                ValidationProblem(
                    f"release history at {commit} has partial identity coverage; "
                    f"missing {sorted(legacy_artifacts)}"
                ),
            ),
        )

    if len(set(history_roots.values())) != 1:
        problems.append(
            ValidationProblem(
                f"release history at {commit} has root mismatch: "
                + ", ".join(
                    f"{name}={history_root}"
                    for name, history_root in history_roots.items()
                )
            )
        )
    release_texts = {release.text for release in releases.values()}
    if len(release_texts) != 1:
        problems.append(
            ValidationProblem(
                f"release history at {commit} has identity mismatch: "
                + ", ".join(
                    f"{name}={release.text}" for name, release in releases.items()
                )
            )
        )
    if len(set(statuses.values())) != 1:
        problems.append(
            ValidationProblem(
                f"release history at {commit} has status mismatch: "
                + ", ".join(f"{name}={status}" for name, status in statuses.items())
            )
        )
    if problems:
        return _StateInspection(state=None, problems=tuple(problems))

    return _StateInspection(
        state=release_state.ArtifactReleaseState(
            history_root=next(iter(history_roots.values())),
            release=next(iter(releases.values())),
            status=next(iter(statuses.values())),
        ),
        problems=(),
    )


def _inspect_states(
    git: git_repository.GitRepository,
    commits: tuple[str, ...],
) -> dict[str, _StateInspection]:
    """Decode carrier state at multiple commits through one batched blob read."""
    unique_commits = tuple(dict.fromkeys(commits))
    queries = tuple(
        query for commit in unique_commits for query in _carrier_queries(commit)
    )
    blobs = git.read_text_blobs(queries)
    return {commit: _decode_state(commit, blobs) for commit in unique_commits}


def _validate_pristine_history_root(
    git: git_repository.GitRepository,
    history_root: str,
) -> list[ValidationProblem]:
    problems: list[ValidationProblem] = []
    historical_artifacts = tuple(
        _HistoricalArtifact(
            name=name,
            query=git_repository.BlobQuery(
                commit=commit,
                path=PurePosixPath(path.as_posix()),
            ),
        )
        for name, path in repository.RELEASE_STATE_PATHS.items()
        for commit in git.commits_touching(
            history_root,
            PurePosixPath(path.as_posix()),
        )
    )
    blobs = git.read_text_blobs(
        tuple(artifact.query for artifact in historical_artifacts)
    )

    for artifact in historical_artifacts:
        text = blobs[artifact.query]
        if text is None:
            continue
        try:
            document = _expect_mapping(
                load_yaml_text(text),
                artifact.query.object_spec,
            )
        except (TypeError, ValueError, yaml.YAMLError) as error:
            problems.append(ValidationProblem(str(error)))
            continue
        if "release_history_root" in document or "release" in document:
            problems.append(
                ValidationProblem(
                    f"release history root {history_root} has prior "
                    f"{artifact.name} release state at {artifact.query.commit}"
                )
            )
    return problems


def _validate_repository_features(
    features: git_repository.HistoryRepositoryFeatures,
) -> list[ValidationProblem]:
    problems: list[ValidationProblem] = []
    if features.replacement_refs:
        problems.append(
            ValidationProblem(
                "release history forbids Git replacement refs: "
                f"{sorted(features.replacement_refs)}"
            )
        )
    if features.shallow:
        problems.append(
            ValidationProblem(
                "release history requires a complete non-shallow repository"
            )
        )
    if features.graft_metadata:
        problems.append(ValidationProblem("release history forbids Git graft metadata"))
    return problems


def validate(
    git: git_repository.GitRepository,
    current: release_state.ArtifactReleaseState,
) -> list[ValidationProblem]:
    """Require monotonic release identity and one-way lifecycle transitions."""
    problems = _validate_repository_features(git.history_repository_features())
    if problems:
        return problems

    topology = git.history_topology()
    repository_origin = current.history_root == release_identity.REPOSITORY_ROOT
    if repository_origin:
        graph = git.reachable_history(topology.heads)
        roots = [commit for commit, parents in graph.items() if not parents]
        if len(roots) != 1:
            return [
                ValidationProblem(
                    "repository-root requires exactly one reachable Git root"
                )
            ]
        history_root = roots[0]
        inspections = _inspect_states(git, tuple(graph))
    else:
        history_root = current.history_root
        root_ancestry_problems = _validate_pristine_history_root(git, history_root)
        problems.extend(root_ancestry_problems)
        if root_ancestry_problems:
            return problems
        graph = git.reachable_history(topology.heads, history_root)
        inspections = _inspect_states(git, (history_root, *graph))
        root_inspection = inspections[history_root]
        problems.extend(root_inspection.problems)
        if root_inspection.state is not None:
            problems.append(
                ValidationProblem(
                    f"release history root {history_root} must predate release state"
                )
            )
        if root_inspection.problems or root_inspection.state is not None:
            return problems

    states: dict[str, release_state.ArtifactReleaseState] = {}
    for commit in graph:
        inspection = inspections[commit]
        problems.extend(inspection.problems)
        if inspection.problems:
            continue
        if inspection.state is None:
            problems.append(
                ValidationProblem(
                    f"commit {commit} in release history has no release state"
                )
            )
            continue
        states[commit] = inspection.state
        if inspection.state.history_root != current.history_root:
            problems.append(
                ValidationProblem(f"release history root changed at {commit}")
            )

    for commit, parents in graph.items():
        state = states.get(commit)
        if state is None:
            continue
        if not parents:
            if repository_origin and commit == history_root:
                continue
            problems.append(
                ValidationProblem(
                    f"release history commit {commit} has no path from "
                    f"history root {history_root}"
                )
            )
            continue
        for parent in parents:
            if not repository_origin and parent == history_root:
                previous = None
            elif parent in graph:
                previous = states.get(parent)
                if previous is None:
                    continue
            else:
                problems.append(
                    ValidationProblem(
                        f"release history commit {commit} crosses boundary "
                        f"through {parent}, expected {history_root}"
                    )
                )
                continue
            problems.extend(
                release_state.validate_transition(
                    previous,
                    state,
                    f"release transition {parent} -> {commit}",
                )
            )

    released_commits: dict[str, set[str]] = {}
    for commit, state in states.items():
        if state.status == "released":
            released_commits.setdefault(state.release.text, set()).add(commit)

    if topology.prospective:
        for predecessor in topology.heads:
            if not repository_origin and predecessor == history_root:
                previous = None
            elif predecessor in graph:
                previous = states.get(predecessor)
                if previous is None:
                    continue
            else:
                problems.append(
                    ValidationProblem(
                        "prospective release state crosses history boundary "
                        f"through {predecessor}, expected {history_root}"
                    )
                )
                continue
            problems.extend(
                release_state.validate_transition(
                    previous,
                    current,
                    f"prospective release transition from {predecessor}",
                )
            )
        if current.status == "released":
            released_commits.setdefault(
                current.release.text,
                set(),
            ).add("prospective index")

    for release, commits in sorted(released_commits.items()):
        if len(commits) > 1:
            problems.append(
                ValidationProblem(
                    f"released identity {release} is bound to multiple commits: "
                    f"{sorted(commits)}"
                )
            )
    return problems

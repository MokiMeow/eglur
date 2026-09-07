"""Canonical specification-release carrier agreement and state transitions."""

from collections.abc import Sequence
from dataclasses import dataclass

from eglur_conformance import release_identity
from eglur_conformance.validation import ValidationProblem


@dataclass(frozen=True)
class ReleaseCarrier:
    """Release identity and lifecycle state carried by one artifact."""

    name: str
    history_root: str
    release: str
    status: str


@dataclass(frozen=True)
class ArtifactReleaseState:
    """One agreed specification-package release identity and lifecycle state."""

    history_root: str
    release: release_identity.ReleaseLabel
    status: str


def agree(
    carriers: Sequence[ReleaseCarrier],
) -> tuple[ArtifactReleaseState | None, list[ValidationProblem]]:
    """Return the common release state or agreement failures."""
    history_roots = {carrier.name: carrier.history_root for carrier in carriers}
    releases = {carrier.name: carrier.release for carrier in carriers}
    statuses = {carrier.name: carrier.status for carrier in carriers}
    problems: list[ValidationProblem] = []

    if len(set(history_roots.values())) != 1:
        problems.append(
            ValidationProblem(
                "artifact release history root mismatch: "
                + ", ".join(
                    f"{name}={history_root}"
                    for name, history_root in history_roots.items()
                )
            )
        )
    if len(set(releases.values())) != 1:
        problems.append(
            ValidationProblem(
                "artifact release identity mismatch: "
                + ", ".join(f"{name}={release}" for name, release in releases.items())
            )
        )
    if len(set(statuses.values())) != 1:
        problems.append(
            ValidationProblem(
                "artifact status mismatch: "
                + ", ".join(f"{name}={status}" for name, status in statuses.items())
            )
        )
    if problems:
        return None, problems

    release = next(iter(releases.values()))
    status = next(iter(statuses.values()))
    history_root = next(iter(history_roots.values()))
    return (
        ArtifactReleaseState(
            history_root=release_identity.validate_history_root(
                history_root,
                "artifact release history root",
            ),
            release=release_identity.parse_release_label(
                release,
                "artifact release",
            ),
            status=status,
        ),
        [],
    )


def validate(
    carriers: Sequence[ReleaseCarrier],
    *,
    require_released: bool = False,
) -> list[ValidationProblem]:
    """Require one carrier state, optionally in released state."""
    state, problems = agree(carriers)
    if require_released and state is not None and state.status != "released":
        problems.append(
            ValidationProblem(
                "released evidence requires artifact_inventory, manifest, "
                "traceability, and open_items_ledger status released"
            )
        )
    return problems


def validate_transition(
    previous: ArtifactReleaseState | None,
    current: ArtifactReleaseState,
    context: str,
) -> list[ValidationProblem]:
    """Validate one historical edge in the release-state machine."""
    if previous is None:
        if current.status != "draft":
            return [
                ValidationProblem(
                    f"{context}: initial release identity "
                    f"{current.release.text} must start in draft"
                )
            ]
        return []

    if current.history_root != previous.history_root:
        return [
            ValidationProblem(
                f"{context}: release history root changed from "
                f"{previous.history_root} to {current.history_root}"
            )
        ]

    if current.release.text == previous.release.text:
        if previous.status == "released":
            return [
                ValidationProblem(
                    f"{context}: release identity {current.release.text} "
                    "was already released and cannot be reused"
                )
            ]
        return []

    problems: list[ValidationProblem] = []
    if not current.release.is_later_than(previous.release):
        problems.append(
            ValidationProblem(
                f"{context}: release identity {current.release.text} must be "
                f"later than {previous.release.text}"
            )
        )
    if current.status != "draft":
        problems.append(
            ValidationProblem(
                f"{context}: new release identity {current.release.text} "
                "must start in draft"
            )
        )
    return problems

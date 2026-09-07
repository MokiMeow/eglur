"""Parse and order canonical Eglur v0.1 specification release labels."""

import re
from dataclasses import dataclass

RELEASE_LABEL_PATTERN = re.compile(
    r"v0\.1\.(?P<patch>0|[1-9][0-9]*)(?:-rc(?P<rc>[1-9][0-9]*))?"
)
RELEASE_HISTORY_ROOT_PATTERN = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})")
REPOSITORY_ROOT = "repository-root"


@dataclass(frozen=True)
class ReleaseLabel:
    """One canonical v0.1 release identity with semantic ordering."""

    text: str
    patch: int
    release_candidate: int | None

    @property
    def ordering_key(self) -> tuple[int, int, int]:
        """Order release candidates before the final release of one patch."""
        if self.release_candidate is None:
            return self.patch, 1, 0
        return self.patch, 0, self.release_candidate

    def is_later_than(self, other: "ReleaseLabel") -> bool:
        """Return whether this identity is strictly later than another."""
        return self.ordering_key > other.ordering_key


def parse_release_label(value: str, context: str) -> ReleaseLabel:
    """Parse a canonical v0.1.x or v0.1.x-rcN release label."""
    match = RELEASE_LABEL_PATTERN.fullmatch(value)
    if match is None:
        raise ValueError(
            f"{context} must be a canonical v0.1.x release or numeric rc label"
        )
    release_candidate = match.group("rc")
    return ReleaseLabel(
        text=value,
        patch=int(match.group("patch")),
        release_candidate=(
            None if release_candidate is None else int(release_candidate)
        ),
    )


def validate_history_root(value: str, context: str) -> str:
    """Validate an origin selector without resolving future commit identity.

    Parameters
    ----------
    value : str
        The literal ``repository-root`` or a full lowercase Git commit ID.
    context : str
        Field location used in validation errors.

    Returns
    -------
    str
        The unchanged canonical selector.

    Raises
    ------
    ValueError
        If the selector is neither the literal nor a full commit ID.
    """
    if (
        value != REPOSITORY_ROOT
        and RELEASE_HISTORY_ROOT_PATTERN.fullmatch(value) is None
    ):
        raise ValueError(
            f"{context} must be repository-root or a full lowercase Git commit ID"
        )
    return value

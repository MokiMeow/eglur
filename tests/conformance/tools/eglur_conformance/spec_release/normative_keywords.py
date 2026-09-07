"""Find lowercase RFC modal keywords in bounded normative prose."""

import re
from dataclasses import dataclass
from pathlib import Path

from eglur_conformance.markdown_document import (
    strip_html_comments as _strip_html_comments,
)

NORMATIVE_PATHS = (
    Path("docs/spec/axes.md"),
    Path("docs/spec/language/implementation-boundary.md"),
    Path("docs/spec/language/tensor-model.md"),
    Path("docs/spec/stdlib/tensor.md"),
)
LOWERCASE_MODAL = re.compile(r"\b(?:must|should|may)\b")
INLINE_CODE = re.compile(r"`+[^`]*`+")


@dataclass(frozen=True)
class KeywordViolation:
    """One lowercase RFC modal in normative prose."""

    path: Path
    line: int
    keyword: str

    def render(self) -> str:
        """Return a source-located validation message."""
        return (
            f"{self.path}:{self.line}: lowercase normative keyword "
            f"{self.keyword!r}; use RFC uppercase or rewrite editorial prose"
        )


def find_violations(
    repo_root: Path,
    relative_paths: tuple[Path, ...] = NORMATIVE_PATHS,
) -> list[KeywordViolation]:
    """Return lowercase RFC modal violations outside quoted or code text."""
    violations: list[KeywordViolation] = []

    for relative_path in relative_paths:
        source = (repo_root / relative_path).read_text(encoding="utf-8")
        fence: str | None = None
        in_html_comment = False

        for line_number, line in enumerate(source.splitlines(), 1):
            stripped = line.lstrip()
            marker = stripped[:3]
            if marker in {"```", "~~~"}:
                if fence is None:
                    fence = marker
                elif marker == fence:
                    fence = None
                continue
            if fence is not None or stripped.startswith(">"):
                continue

            visible, in_html_comment = _strip_html_comments(line, in_html_comment)
            visible = INLINE_CODE.sub("", visible)
            for match in LOWERCASE_MODAL.finditer(visible):
                violations.append(
                    KeywordViolation(
                        path=relative_path,
                        line=line_number,
                        keyword=match.group(0),
                    )
                )

    return violations

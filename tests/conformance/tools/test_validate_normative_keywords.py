"""Tests for the bounded normative RFC keyword audit."""

import tempfile
import unittest
from pathlib import Path

from eglur_conformance.spec_release import normative_keywords


class NormativeKeywordValidationTests(unittest.TestCase):
    """Keep normative modals uppercase without scanning quoted examples."""

    def find(self, source: str) -> list[normative_keywords.KeywordViolation]:
        """Validate one synthetic normative document."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = Path("normative.md")
            (root / path).write_text(source, encoding="utf-8")
            return normative_keywords.find_violations(root, (path,))

    def test_rejects_each_lowercase_modal_with_source_location(self) -> None:
        violations = self.find(
            "Implementations must reject this.\n"
            "Callers should validate it.\n"
            "Backends may optimize it.\n"
        )

        self.assertEqual(
            [(item.line, item.keyword) for item in violations],
            [(1, "must"), (2, "should"), (3, "may")],
        )
        self.assertIn("normative.md:1", violations[0].render())

    def test_accepts_uppercase_rfc_keywords(self) -> None:
        self.assertEqual(
            self.find("Implementations MUST reject and MAY optimize this.\n"),
            [],
        )

    def test_ignores_code_inline_code_quotes_and_html_comments(self) -> None:
        source = """```text
implementations must reject
```
Use `a value that may vary` as the example.
> A quoted draft should remain unchanged.
<!-- implementations must not scan this -->
Implementations MUST reject this.
"""
        self.assertEqual(self.find(source), [])


if __name__ == "__main__":
    unittest.main()

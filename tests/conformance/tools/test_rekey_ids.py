"""Adversarial tests for semantic conformance-ID rekeying."""

import io
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path

from eglur_conformance.suite import rekey


class RekeyIdsTests(unittest.TestCase):
    """Require structured discovery and format-preserving ID rewrites."""

    def test_collects_yaml_fields_without_key_order_or_line_layout_assumptions(
        self,
    ) -> None:
        source = """\
tests:
  - {title: First title, id: 'TYC-001'}
  - title: >-
      Second title
    id: "TYC-002"
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "suite.yaml"
            path.write_text(source, encoding="utf-8")

            entries = rekey.collect_entries(path)
            mapping = rekey.assign_new_ids(entries)

            self.assertEqual(
                mapping,
                {
                    "TYC-001": "TYC-first-title",
                    "TYC-002": "TYC-second-title",
                },
            )
            self.assertTrue(rekey.rewrite_suite_file(path, mapping))
            self.assertEqual(
                path.read_text(encoding="utf-8"),
                source.replace("'TYC-001'", "'TYC-first-title'").replace(
                    '"TYC-002"',
                    '"TYC-second-title"',
                ),
            )

    def test_duplicate_source_id_fails_before_any_rewrite(self) -> None:
        entries = [
            rekey.TestEntry(Path("a.yaml"), "TYC-001", "TYC", "First"),
            rekey.TestEntry(Path("b.yaml"), "TYC-001", "TYC", "Second"),
        ]

        with self.assertRaisesRegex(ValueError, "duplicate test id: TYC-001"):
            rekey.assign_new_ids(entries)

    def test_empty_reference_mapping_is_a_noop(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(
                rekey.rewrite_references(Path(directory), {}),
                [],
            )

    def test_reference_rewrite_matches_only_complete_ids(self) -> None:
        source = """\
TYC-001
(TYC-001)
TYC-001-extra
prefix-TYC-001
TYC-001_suffix
"""
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            path = repo_root / "notes.md"
            path.write_text(source, encoding="utf-8")

            changed = rekey.rewrite_references(
                repo_root,
                {"TYC-001": "TYC-semantic"},
            )

            self.assertEqual(changed, [path])
            self.assertEqual(
                path.read_text(encoding="utf-8"),
                source.replace("TYC-001\n", "TYC-semantic\n", 1).replace(
                    "(TYC-001)",
                    "(TYC-semantic)",
                ),
            )

    def test_empty_or_invalid_suite_selection_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            empty = repo_root / "empty"
            empty.mkdir()
            excluded = repo_root / "manifest.yaml"
            excluded.write_text("suites: []\n", encoding="utf-8")
            no_tests = repo_root / "suite.yaml"
            no_tests.write_text("tests: []\n", encoding="utf-8")

            for path, message in (
                (repo_root / "missing", "no suite files found"),
                (empty, "no suite files found"),
                (excluded, "no suite files found"),
                (no_tests, "suite files contain no tests"),
            ):
                args = rekey.build_parser().parse_args(
                    ["--repo-root", str(repo_root), str(path)]
                )
                with (
                    self.subTest(path=path),
                    redirect_stderr(io.StringIO()) as stderr,
                ):
                    self.assertEqual(rekey.run(args), 2)
                    self.assertIn(message, stderr.getvalue())


if __name__ == "__main__":
    unittest.main()

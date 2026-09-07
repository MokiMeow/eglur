"""Adversarial tests for OPEN-item ledger source binding."""

import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

from eglur_conformance.markdown_document import visible_markdown_lines
from eglur_conformance.spec_release import open_items
from eglur_conformance.validation import ValidationProblem


class OpenItemLedgerTests(unittest.TestCase):
    """Reject malformed ledger/source relationships without layout accidents."""

    def extract_source_items(
        self,
        repo_root: Path,
        rel_path: str,
    ) -> tuple[open_items.SourceItem, ...]:
        """Load visible source lines and apply the pure item parser."""
        source_path = open_items.resolve_source_path(repo_root, rel_path)
        return open_items.extract_source_items(
            rel_path,
            visible_markdown_lines(source_path),
        )

    def validate_ledger(
        self,
        repo_root: Path,
        ledger: open_items.Ledger,
    ) -> list[ValidationProblem]:
        """Compose source inspection with pure ledger validation."""
        inspection = open_items.inspect_sources(repo_root, ledger.sources)
        return open_items.validate_ledger(ledger, inspection)

    def make_ledger(self) -> open_items.Ledger:
        """Return one valid two-item synthetic ledger."""
        return open_items.Ledger(
            version="0.1",
            release_history_root="a" * 40,
            release="v0.1.0",
            status="draft",
            purpose="test",
            classification_keys=(
                "baseline_must_close",
                "deferred_nonbaseline",
            ),
            sources=(
                open_items.SourceConfig(
                    path="open-items.md",
                    item_count=2,
                ),
            ),
            items=(
                open_items.LedgerItem(
                    item_id="OI-001",
                    source_path="open-items.md",
                    classification="deferred_nonbaseline",
                ),
                open_items.LedgerItem(
                    item_id="OI-002",
                    source_path="open-items.md",
                    classification="baseline_must_close",
                ),
            ),
            summary={
                "total_items": 2,
                "baseline_must_close_count": 1,
                "deferred_nonbaseline_count": 1,
                "unclassified_count": 0,
                "baseline_must_close_open_count": 1,
            },
        )

    def test_ignores_numbered_text_inside_fences_and_html_comments(self) -> None:
        source = """\
1. **`OI-001`** - First
```
2. **`OI-HIDDEN-001`** - Not an item
```
<!--
```
2. **`OI-HIDDEN-002`** - Also not an item
```
-->
2. **`OI-002`** - Second
"""
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            (repo_root / "open-items.md").write_text(source, encoding="utf-8")

            items = self.extract_source_items(
                repo_root,
                "open-items.md",
            )

        self.assertEqual(
            [(item.number, item.item_id, item.title) for item in items],
            [(1, "OI-001", "First"), (2, "OI-002", "Second")],
        )

    def test_rejects_unterminated_hidden_regions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            path = repo_root / "open-items.md"
            for source, message in (
                (
                    "1. **`OI-001`** - First\n```\n",
                    "unterminated Markdown code fence",
                ),
                (
                    "1. **`OI-001`** - First\n<!--\n",
                    "unterminated HTML comment",
                ),
            ):
                with self.subTest(source=source):
                    path.write_text(source, encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, message):
                        self.extract_source_items(
                            repo_root,
                            "open-items.md",
                        )

    def test_rejects_source_paths_outside_the_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary_root = Path(directory)
            repo_root = temporary_root / "repo"
            repo_root.mkdir()
            outside = temporary_root / "outside.md"
            outside.write_text(
                "1. **`OI-001`** - Outside\n",
                encoding="utf-8",
            )
            valid = self.make_ledger()

            for source_path in (
                str(outside),
                "../outside.md",
            ):
                ledger = replace(
                    valid,
                    sources=(replace(valid.sources[0], path=source_path),),
                    items=tuple(
                        replace(item, source_path=source_path) for item in valid.items
                    ),
                )
                with self.subTest(source_path=source_path):
                    problems = self.validate_ledger(
                        repo_root,
                        ledger,
                    )
                    self.assertTrue(
                        any("OPEN source path" in item.message for item in problems)
                    )

            (repo_root / "open-items.md").symlink_to(outside)
            problems = self.validate_ledger(repo_root, valid)
            self.assertTrue(
                any("escapes repository root" in item.message for item in problems)
            )

            (repo_root / "open-items.md").unlink()
            (repo_root / "open-items.md").symlink_to("open-items.md")
            problems = self.validate_ledger(repo_root, valid)
            self.assertTrue(
                any("cannot be resolved" in item.message for item in problems)
            )

    def test_rejects_each_malformed_ledger_relationship(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            (repo_root / "open-items.md").write_text(
                "1. **`OI-001`** - First\n2. **`OI-002`** - Second\n",
                encoding="utf-8",
            )
            valid = self.make_ledger()
            self.assertEqual(
                self.validate_ledger(repo_root, valid),
                [],
            )

            malformed = (
                replace(
                    valid,
                    sources=(replace(valid.sources[0], item_count=1),),
                ),
                replace(
                    valid,
                    items=(
                        valid.items[0],
                        replace(valid.items[1], item_id="OI-001"),
                    ),
                ),
                replace(
                    valid,
                    items=(
                        valid.items[0],
                        replace(valid.items[1], item_id="OI-999"),
                    ),
                ),
                replace(
                    valid,
                    items=(
                        replace(valid.items[0], classification="unknown"),
                        valid.items[1],
                    ),
                ),
                replace(
                    valid,
                    summary={**valid.summary, "total_items": 1},
                ),
            )

            for ledger in malformed:
                with self.subTest(ledger=ledger):
                    self.assertTrue(self.validate_ledger(repo_root, ledger))

    def test_rejects_forged_closed_count_for_present_baseline_item(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            (repo_root / "open-items.md").write_text(
                "1. **`OI-001`** - First\n2. **`OI-002`** - Second\n",
                encoding="utf-8",
            )
            valid = self.make_ledger()
            forged = replace(
                valid,
                summary={
                    **valid.summary,
                    "baseline_must_close_open_count": 0,
                },
            )

            problems = self.validate_ledger(repo_root, forged)

        self.assertTrue(
            any(
                "summary.baseline_must_close_open_count is 0, expected 1"
                in item.message
                for item in problems
            )
        )

    def test_rejects_one_sided_resolution_updates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            source_path = repo_root / "open-items.md"
            source_path.write_text(
                "1. **`OI-001`** - First\n2. **`OI-002`** - Second\n",
                encoding="utf-8",
            )
            valid = self.make_ledger()
            ledger_only = replace(
                valid,
                items=(valid.items[0],),
                summary={
                    "total_items": 1,
                    "baseline_must_close_count": 0,
                    "deferred_nonbaseline_count": 1,
                    "unclassified_count": 0,
                    "baseline_must_close_open_count": 0,
                },
            )
            ledger_only_problems = self.validate_ledger(repo_root, ledger_only)

            source_path.write_text(
                "1. **`OI-001`** - First\n",
                encoding="utf-8",
            )
            source_only = replace(
                valid,
                sources=(replace(valid.sources[0], item_count=1),),
            )
            source_only_problems = self.validate_ledger(repo_root, source_only)

        self.assertTrue(
            any("missing ledger item" in item.message for item in ledger_only_problems)
        )
        self.assertTrue(
            any("extra ledger item" in item.message for item in source_only_problems)
        )

    def test_source_reordering_preserves_stable_id_binding(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            (repo_root / "open-items.md").write_text(
                "1. **`OI-002`** - Second\n2. **`OI-001`** - First\n",
                encoding="utf-8",
            )

            problems = self.validate_ledger(
                repo_root,
                self.make_ledger(),
            )

        self.assertEqual(problems, [])

    def test_rejects_duplicate_source_ids(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            (repo_root / "open-items.md").write_text(
                "1. **`OI-001`** - First\n2. **`OI-001`** - Second\n",
                encoding="utf-8",
            )

            problems = self.validate_ledger(
                repo_root,
                self.make_ledger(),
            )

        self.assertTrue(
            any(
                "source OPEN item id OI-001 appears 2 times" in item.message
                for item in problems
            )
        )

    def test_rejects_top_level_items_without_well_formed_stable_ids(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            path = repo_root / "open-items.md"
            for source in (
                "1. First\n",
                "1. **`lowercase-id`** - First\n",
                "1. **`OI-001`** - \n",
            ):
                with self.subTest(source=source):
                    path.write_text(source, encoding="utf-8")
                    with self.assertRaisesRegex(
                        ValueError,
                        "top-level OPEN item must use",
                    ):
                        self.extract_source_items(
                            repo_root,
                            "open-items.md",
                        )


if __name__ == "__main__":
    unittest.main()

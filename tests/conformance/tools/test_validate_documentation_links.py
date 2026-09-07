"""Boundary tests for tracked Markdown link validation."""

import io
import subprocess
import tempfile
import unittest
from argparse import Namespace
from contextlib import redirect_stdout
from pathlib import Path

import validate_documentation_links


def initialize_repository(repo_root: Path) -> None:
    """Create a Git index for the synthetic documentation checkout."""
    subprocess.run(["git", "init", "-q"], cwd=repo_root, check=True)
    subprocess.run(["git", "add", "."], cwd=repo_root, check=True)


class DocumentationLinkCliTests(unittest.TestCase):
    """Keep Git discovery, worktree loading, and CLI results fail-closed."""

    def run_validator(self, repo_root: Path) -> tuple[int, str]:
        """Run the adapter and capture its user-visible report."""
        output = io.StringIO()
        with redirect_stdout(output):
            status = validate_documentation_links.run(Namespace(repo_root=repo_root))
        return status, output.getvalue()

    def test_accepts_one_complete_tracked_documentation_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            docs = repo_root / "docs"
            docs.mkdir()
            (repo_root / "README.md").write_text(
                "[guide](docs/guide.md#guide)\n[data](data.yaml)\n[docs](docs/)\n",
                encoding="utf-8",
            )
            (repo_root / "data.yaml").write_text("value: 1\n", encoding="utf-8")
            (docs / "guide.md").write_text("# Guide\n", encoding="utf-8")
            initialize_repository(repo_root)

            status, output = self.run_validator(repo_root)

        self.assertEqual(status, 0)
        self.assertEqual(
            output,
            "documentation link validation passed: 2 Markdown files.\n",
        )

    def test_rejects_untracked_and_deleted_targets(self) -> None:
        for target_state in ("untracked", "deleted"):
            with self.subTest(target_state=target_state):
                with tempfile.TemporaryDirectory() as directory:
                    repo_root = Path(directory)
                    (repo_root / "README.md").write_text(
                        "[target](target.md)\n",
                        encoding="utf-8",
                    )
                    if target_state == "deleted":
                        (repo_root / "target.md").write_text(
                            "# Target\n",
                            encoding="utf-8",
                        )
                    initialize_repository(repo_root)
                    if target_state == "untracked":
                        (repo_root / "target.md").write_text(
                            "# Target\n",
                            encoding="utf-8",
                        )
                    else:
                        (repo_root / "target.md").unlink()

                    status, output = self.run_validator(repo_root)

                self.assertEqual(status, 1)
                expected = (
                    "target is not tracked and present"
                    if target_state == "untracked"
                    else "cannot load tracked Markdown source target.md"
                )
                self.assertIn(expected, output)

    def test_rejects_repository_subdirectory_as_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            docs = repo_root / "docs"
            docs.mkdir()
            (docs / "guide.md").write_text("# Guide\n", encoding="utf-8")
            initialize_repository(repo_root)

            status, output = self.run_validator(docs)

        self.assertEqual(status, 1)
        self.assertIn("not the Git worktree root", output)

    def test_rejects_symlinked_and_non_utf8_markdown_sources(self) -> None:
        cases = ("symlink", "non-utf8")
        for source_kind in cases:
            with self.subTest(source_kind=source_kind):
                with tempfile.TemporaryDirectory() as directory:
                    temporary_root = Path(directory)
                    repo_root = temporary_root / "repo"
                    repo_root.mkdir()
                    source_path = repo_root / "README.md"
                    if source_kind == "symlink":
                        outside = temporary_root / "outside.md"
                        outside.write_text("# Outside\n", encoding="utf-8")
                        source_path.symlink_to(outside)
                    else:
                        source_path.write_bytes(b"\xff\n")
                    initialize_repository(repo_root)

                    status, output = self.run_validator(repo_root)

                self.assertEqual(status, 1)
                self.assertIn("cannot load tracked Markdown source", output)


if __name__ == "__main__":
    unittest.main()

"""CLI boundary tests for explicit repository-root resolution."""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


class ToolRepoRootTests(unittest.TestCase):
    """Run release tools from outside an arbitrarily located checkout."""

    temporary_directory: tempfile.TemporaryDirectory[str]
    checkout: Path
    outside_cwd: Path

    @classmethod
    def setUpClass(cls) -> None:
        cls.temporary_directory = tempfile.TemporaryDirectory()
        temporary_root = Path(cls.temporary_directory.name)
        cls.checkout = temporary_root / "nested" / "checkout"
        cls.outside_cwd = temporary_root / "outside"
        cls.outside_cwd.mkdir()

        tracked = subprocess.run(
            ["git", "ls-files", "-z"],
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        for relative in tracked.stdout.split("\0"):
            if not relative:
                continue
            destination = cls.checkout / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO_ROOT / relative, destination)
        subprocess.run(
            ["git", "init", "-q"],
            cwd=cls.checkout,
            check=True,
        )
        subprocess.run(
            ["git", "add", "."],
            cwd=cls.checkout,
            check=True,
        )

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temporary_directory.cleanup()

    def run_tool(
        self,
        name: str,
        *arguments: str,
    ) -> subprocess.CompletedProcess[str]:
        """Run one copied tool with an explicit checkout root."""
        return subprocess.run(
            [
                sys.executable,
                str(self.checkout / "tests/conformance/tools" / name),
                "--repo-root",
                str(self.checkout),
                *arguments,
            ],
            cwd=self.outside_cwd,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_read_only_tools_ignore_process_cwd_and_checkout_depth(self) -> None:
        commands = (
            ("validate_artifact_inventory.py", ()),
            ("validate_conformance_schema.py", ()),
            ("gen_traceability.py", ("--check",)),
            ("validate_open_items.py", ()),
            ("validate_normative_keywords.py", ()),
            ("validate_documentation_links.py", ()),
        )

        for name, arguments in commands:
            with self.subTest(name=name):
                completed = self.run_tool(name, *arguments)
                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr or completed.stdout,
                )

    def test_rekey_manifest_root_is_relative_to_explicit_repo_root(self) -> None:
        for arguments in ((), ("tests/conformance",)):
            with self.subTest(arguments=arguments):
                completed = self.run_tool("rekey_ids.py", *arguments)

                self.assertEqual(
                    completed.returncode,
                    0,
                    completed.stderr or completed.stdout,
                )
                self.assertIn(" -> ", completed.stdout)
                self.assertIn("Dry run only.", completed.stdout)

    def test_release_gate_adapter_imports_from_copied_checkout(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(self.checkout / "tests/conformance/tools/validate_release_gate.py"),
                "implementation",
            ],
            cwd=self.outside_cwd,
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(completed.returncode, 2)
        self.assertIn(
            "implementation conformance: UNAVAILABLE",
            completed.stderr,
        )


if __name__ == "__main__":
    unittest.main()

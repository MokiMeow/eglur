"""Boundary and orchestration tests for explicit v0.1 evidence verdicts."""

import io
import subprocess
import tempfile
import tomllib
import unittest
from contextlib import chdir, redirect_stderr, redirect_stdout
from dataclasses import replace
from pathlib import Path
from unittest.mock import patch

TOOLS_DIR = Path(__file__).resolve().parent

from eglur_conformance.spec_release import cli as release_cli
from eglur_conformance.spec_release import gate, git_repository, open_items, repository
from eglur_conformance.validation import ValidationProblem


class ReleaseGateCliBoundaryTests(unittest.TestCase):
    """Unavailable release operations cannot consume evidence or issue verdicts."""

    def test_requires_an_explicit_operation(self) -> None:
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as caught:
            release_cli.build_parser().parse_args([])
        self.assertEqual(caught.exception.code, 2)

    def test_unavailable_modes_reject_evidence_arguments(self) -> None:
        for mode in ("spec-package", "implementation"):
            for flag in (
                "--signoff",
                "--repo-root",
                "--suite-results",
                "--certificate",
            ):
                with self.subTest(mode=mode, flag=flag):
                    with (
                        redirect_stderr(io.StringIO()),
                        self.assertRaises(SystemExit) as caught,
                    ):
                        release_cli.build_parser().parse_args([mode, flag, "absent"])
                    self.assertEqual(caught.exception.code, 2)

    def test_unavailable_modes_do_not_require_a_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as directory, chdir(directory):
            for mode, label in (
                ("spec-package", "spec-package release readiness"),
                ("implementation", "implementation conformance"),
            ):
                with self.subTest(mode=mode):
                    args = release_cli.build_parser().parse_args([mode])
                    with (
                        redirect_stdout(io.StringIO()) as stdout,
                        redirect_stderr(io.StringIO()) as stderr,
                    ):
                        status = release_cli.run(args)
                    self.assertEqual(status, 2)
                    self.assertEqual(stdout.getvalue(), "")
                    self.assertIn(f"{label}: UNAVAILABLE", stderr.getvalue())
                    self.assertNotIn(f"{label}: GO", stderr.getvalue())
                    self.assertNotIn(f"{label}: PASS", stderr.getvalue())


class ReleaseGatePolicyTests(unittest.TestCase):
    """Exercise gate-owned checks without routing through the CLI adapter."""

    def test_checked_pixi_tasks_exist(self) -> None:
        repo_root = TOOLS_DIR.parents[2]
        pixi_tasks = tomllib.loads((repo_root / "pixi.toml").read_text())["tasks"]
        checked_tasks = (
            label.rsplit(maxsplit=1)[-1]
            for label in gate.STATIC_CHECK_LABELS
            if label.startswith("pixi -q run --locked ")
        )
        for task in checked_tasks:
            with self.subTest(task=task):
                self.assertIn(task, pixi_tasks)

    def test_open_item_readiness_rejects_valid_baseline_blocker(self) -> None:
        repo_root = TOOLS_DIR.parents[2]
        ledger = repository.load_snapshot(repo_root).ledger
        blocked = replace(
            ledger,
            items=(
                replace(
                    ledger.items[0],
                    classification="baseline_must_close",
                ),
                *ledger.items[1:],
            ),
            summary={
                **ledger.summary,
                "baseline_must_close_count": 1,
                "deferred_nonbaseline_count": len(ledger.items) - 1,
                "baseline_must_close_open_count": 1,
            },
        )
        inspection = open_items.inspect_sources(repo_root, blocked.sources)

        self.assertEqual(open_items.validate_ledger(blocked, inspection), [])
        self.assertEqual(
            [problem.message for problem in gate.validate_open_item_readiness(blocked)],
            [
                (
                    "open-items ledger summary.baseline_must_close_open_count is "
                    "1, expected 0"
                )
            ],
        )

    def test_schema_validator_gate_requires_nonzero_test_discovery(self) -> None:
        cases = (
            ("", "did not report a test count"),
            ("Ran 0 tests in 0.000s\n\nOK", "discovered zero tests"),
        )
        for output, expected_message in cases:
            with (
                self.subTest(output=output),
                patch.object(
                    gate.subprocess,
                    "run",
                    return_value=subprocess.CompletedProcess(
                        args=[],
                        returncode=0,
                        stdout="",
                        stderr=output,
                    ),
                ),
            ):
                problems = gate.validate_schema_validator_tests(TOOLS_DIR.parents[2])

            self.assertEqual(len(problems), 1)
            self.assertIn(expected_message, problems[0].message)

        with patch.object(
            gate.subprocess,
            "run",
            return_value=subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout="",
                stderr="Ran 1 test in 0.001s\n\nOK",
            ),
        ):
            problems = gate.validate_schema_validator_tests(TOOLS_DIR.parents[2])

        self.assertEqual(problems, [])

    def test_static_gate_rejects_invalid_effect_summary_catalog(self) -> None:
        with patch.object(
            gate.effect_codec,
            "load_effect_catalog",
            side_effect=ValueError("callback metadata mismatch"),
        ):
            problems = gate.validate_effect_summary_catalog(TOOLS_DIR.parents[2])

        self.assertEqual(len(problems), 1)
        self.assertIn("callback metadata mismatch", problems[0].message)

    def test_static_gate_rejects_unclassified_inventory_artifact(self) -> None:
        repo_root = TOOLS_DIR.parents[2]
        snapshot = repository.load_snapshot(repo_root)
        broken_inventory = replace(
            snapshot.inventory,
            artifacts=tuple(
                rule
                for rule in snapshot.inventory.artifacts
                if rule.selector != "docs/spec/*.md"
            ),
        )
        broken_snapshot = replace(snapshot, inventory=broken_inventory)
        with (
            patch.object(
                gate.artifact_inventory,
                "validate_checkout_binding",
                return_value=[],
            ),
            patch.object(gate.suite_schema, "validate_all", return_value=[]),
            patch.object(
                gate.registry_cache_repository,
                "validate_repository",
                return_value=[],
            ),
            patch.object(
                gate,
                "validate_schema_validator_tests",
                return_value=[],
            ),
            patch.object(
                gate,
                "validate_traceability_freshness",
                return_value=[],
            ),
            patch.object(gate.open_items, "validate_ledger", return_value=[]),
            patch.object(
                gate.normative_keywords,
                "find_violations",
                return_value=[],
            ),
            patch.object(gate.history, "validate", return_value=[]),
        ):
            problems = gate.validate_static(
                git_repository.GitRepository(repo_root),
                broken_snapshot,
            )

        self.assertTrue(
            any(
                problem.message == "unclassified artifact: docs/spec/CONFORMANCE.md"
                for problem in problems
            )
        )

    def test_static_gate_preserves_canonical_validation_problems(self) -> None:
        repo_root = TOOLS_DIR.parents[2]
        snapshot = repository.load_snapshot(repo_root)
        suite_problem = ValidationProblem("suite invalid")
        registry_problem = ValidationProblem("registry-cache profile invalid")
        with (
            patch.object(
                gate.artifact_inventory,
                "validate_checkout_binding",
                return_value=[],
            ),
            patch.object(
                gate.artifact_inventory,
                "validate_inventory",
                return_value=[],
            ),
            patch.object(
                gate.suite_schema,
                "validate_all",
                return_value=[suite_problem],
            ),
            patch.object(
                gate.registry_cache_repository,
                "validate_repository",
                return_value=[registry_problem],
            ),
            patch.object(
                gate,
                "validate_schema_validator_tests",
                return_value=[],
            ),
            patch.object(
                gate,
                "validate_traceability_freshness",
                return_value=[],
            ),
            patch.object(gate.open_items, "validate_ledger", return_value=[]),
            patch.object(
                gate.normative_keywords,
                "find_violations",
                return_value=[],
            ),
            patch.object(gate.history, "validate", return_value=[]),
        ):
            problems = gate.validate_static(
                git_repository.GitRepository(repo_root),
                snapshot,
            )

        self.assertIs(problems[0], suite_problem)
        self.assertIs(problems[1], registry_problem)


if __name__ == "__main__":
    unittest.main()

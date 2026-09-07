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
    """Keep spec release readiness distinct from implementation conformance."""

    def assert_parse_failure(self, arguments: list[str]) -> None:
        """Require argparse to reject an incomplete or cross-mode command."""
        with (
            redirect_stderr(io.StringIO()),
            self.assertRaises(SystemExit) as caught,
        ):
            release_cli.build_parser().parse_args(arguments)
        self.assertEqual(caught.exception.code, 2)

    def test_requires_an_explicit_verdict_kind(self) -> None:
        self.assert_parse_failure([])

    def test_spec_package_mode_requires_only_its_signoff(self) -> None:
        self.assert_parse_failure(["spec-package"])
        self.assert_parse_failure(
            [
                "spec-package",
                "--signoff",
                "spec-release.yaml",
                "--suite-results",
                "suite-results.yaml",
            ]
        )

    def test_implementation_mode_rejects_unreleased_evidence_arguments(self) -> None:
        self.assert_parse_failure(
            [
                "implementation",
                "--suite-results",
                "suite-results.yaml",
            ]
        )
        self.assert_parse_failure(
            [
                "implementation",
                "--certificate",
                "implementation-certificate.yaml",
            ]
        )

    def test_spec_package_success_reports_only_go(self) -> None:
        args = release_cli.build_parser().parse_args(
            ["spec-package", "--signoff", "spec-release.yaml"]
        )
        evaluation = gate.GateEvaluation(
            problems=(),
            checked=("static-check", "spec-release.yaml"),
        )
        with (
            patch.object(
                release_cli.gate,
                "evaluate_spec_package",
                return_value=evaluation,
            ),
            redirect_stdout(io.StringIO()) as stdout,
        ):
            status = release_cli.run(args)

        self.assertEqual(status, 0)
        self.assertIn("spec-package release readiness: GO", stdout.getvalue())
        self.assertIn("- static-check", stdout.getvalue())
        self.assertNotIn("implementation conformance: PASS", stdout.getvalue())

    def test_spec_package_resolves_signoff_from_explicit_repo_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary_root = Path(directory)
            repo_root = temporary_root / "nested" / "checkout"
            outside_cwd = temporary_root / "outside"
            repo_root.mkdir(parents=True)
            outside_cwd.mkdir()
            args = release_cli.build_parser().parse_args(
                [
                    "spec-package",
                    "--repo-root",
                    str(repo_root),
                    "--signoff",
                    "evidence/signoff.yaml",
                ]
            )
            with (
                chdir(outside_cwd),
                patch.object(
                    release_cli.gate,
                    "evaluate_spec_package",
                    return_value=gate.GateEvaluation(problems=(), checked=()),
                ) as evaluate,
                redirect_stdout(io.StringIO()),
            ):
                status = release_cli.run(args)

        self.assertEqual(status, 0)
        self.assertEqual(
            evaluate.call_args.args,
            (
                repo_root.resolve(),
                repo_root.resolve() / "evidence/signoff.yaml",
                "evidence/signoff.yaml",
            ),
        )

    def test_implementation_mode_fails_closed_without_evidence_profile(self) -> None:
        args = release_cli.build_parser().parse_args(["implementation"])
        with redirect_stderr(io.StringIO()) as stderr:
            status = release_cli.run(args)

        self.assertEqual(status, 2)
        self.assertIn("implementation conformance: UNAVAILABLE", stderr.getvalue())
        self.assertNotIn("implementation conformance: PASS", stderr.getvalue())

    def test_boundary_error_reports_scoped_no_go(self) -> None:
        args = release_cli.build_parser().parse_args(
            ["spec-package", "--signoff", "spec-release.yaml"]
        )
        with (
            patch.object(
                release_cli.gate,
                "evaluate_spec_package",
                side_effect=TypeError("inventory must be a mapping"),
            ),
            redirect_stderr(io.StringIO()) as stderr,
        ):
            status = release_cli.run(args)

        self.assertEqual(status, 1)
        self.assertIn("spec-package release readiness: NO-GO", stderr.getvalue())
        self.assertIn("inventory must be a mapping", stderr.getvalue())

    def test_validation_problems_use_header_and_bullets(self) -> None:
        args = release_cli.build_parser().parse_args(
            ["spec-package", "--signoff", "spec-release.yaml"]
        )
        evaluation = gate.GateEvaluation(
            problems=(ValidationProblem("first failure"),),
            checked=(),
        )
        with (
            patch.object(
                release_cli.gate,
                "evaluate_spec_package",
                return_value=evaluation,
            ),
            redirect_stderr(io.StringIO()) as stderr,
        ):
            status = release_cli.run(args)

        self.assertEqual(status, 1)
        self.assertEqual(
            stderr.getvalue(),
            "spec-package release readiness: NO-GO:\n- first failure\n",
        )


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

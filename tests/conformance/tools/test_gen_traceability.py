"""Regression tests for diagnostic contract traceability."""

import tempfile
import unittest
from pathlib import Path
from typing import ClassVar
from unittest.mock import patch

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.diagnostics.registry import load_diagnostic_registry
from eglur_conformance.markdown_document import visible_markdown_lines
from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import model as suite_model
from eglur_conformance.traceability import analysis as traceability_analysis
from eglur_conformance.traceability import codec as traceability_codec
from eglur_conformance.traceability import repository as traceability_repository


class TraceabilityContractTests(unittest.TestCase):
    """Exercise fixture-to-contract drift detection."""

    contracts: ClassVar[dict[str, diagnostic_contracts.DiagnosticContract]]
    channel_registry: ClassVar[diagnostic_model.DiagnosticRegistry]
    codes: ClassVar[set[str]]
    suite_entry: ClassVar[suite_model.SuiteEntry]

    @classmethod
    def setUpClass(cls) -> None:
        """Load the canonical contracts once for synthetic fixture checks."""
        cls.contracts = diagnostic_contracts.load_contracts(
            REPO_ROOT / diagnostic_contracts.DEFAULT_CONTRACT_PATH
        )
        cls.channel_registry = load_diagnostic_registry(
            REPO_ROOT / "docs/spec/errors",
            cls.contracts,
        )
        cls.codes = set(cls.contracts)
        cls.suite_entry = suite_model.SuiteEntry(
            suite_id="synthetic",
            file="synthetic.yaml",
            participation=suite_model.BaselineParticipation(),
        )

    def evidence_for(
        self,
        document: dict[str, diagnostic_contracts.YamlValue],
    ) -> tuple[set[str], set[str], list[str]]:
        """Return coverage maps and failures for one synthetic suite document."""
        suite = suite_model.ConformanceSuite(
            entry=self.suite_entry,
            document=suite_codec.decode_suite_document(
                document,
                self.suite_entry.file,
            ),
        )
        asserted, mentioned, failures = traceability_analysis._collect_test_evidence(
            suite,
            self.codes,
            self.contracts,
            self.channel_registry,
        )
        return set(asserted), set(mentioned), [failure.field for failure in failures]

    def failures_for(
        self,
        diagnostic: dict[str, diagnostic_contracts.YamlValue],
    ) -> list[str]:
        """Return contract-failure fields for one synthetic diagnostic."""
        document: dict[str, diagnostic_contracts.YamlValue] = {
            "tests": [
                {
                    "id": "SYN-001",
                    "category": "diagnostic",
                    "title": "synthetic diagnostic",
                    "code": 'fn main!() -> Res[Str, Str] { yield Ok("x") }',
                    "expect": {"compile_error": diagnostic},
                }
            ]
        }
        _, _, failures = self.evidence_for(document)
        return failures

    def runtime_code_result_for(
        self,
        runtime_code: str,
    ) -> tuple[set[str], set[str], list[str]]:
        """Return coverage maps and failures for one test-runner runtime code."""
        document: dict[str, diagnostic_contracts.YamlValue] = {
            "tests": [
                {
                    "id": "SYN-001",
                    "category": "runtime",
                    "title": "synthetic runtime abort",
                    "code": "fn aborts!() -> Unit { abort!() }",
                    "runner": "test",
                    "expect": {
                        "test_run": {
                            "order": [{"module": "root", "name": "aborts"}],
                            "passed": [],
                            "skipped": [],
                            "failed": [
                                {
                                    "module": "root",
                                    "name": "aborts",
                                    "kind": "runtime_abort",
                                    "message_contains": ["runtime abort"],
                                    "runtime_code": runtime_code,
                                }
                            ],
                        }
                    },
                }
            ]
        }
        return self.evidence_for(document)

    def test_accepts_static_one_of_and_template_contracts(self) -> None:
        one_of = self.failures_for(
            {
                "code": "EFFECT_VIOLATION",
                "message_contains": [],
                "message_contains_any": [["non-bang", "Fn", "pure"]],
                "data_keys": [],
            }
        )
        template = self.failures_for(
            {
                "code": "TRAVERSE_RES_FAIL",
                "message_contains": ["index 3"],
                "data_keys": ["index"],
            }
        )

        self.assertEqual(one_of, [])
        self.assertEqual(template, [])

    def test_rejects_required_fixture_message_and_help_overreach(self) -> None:
        failures = self.failures_for(
            {
                "code": "EFFECT_SIGNATURE",
                "message_contains": ["&mut Self", "method receiver"],
                "help_contains": ["make the method bang"],
                "data_keys": [],
            }
        )

        self.assertIn("message_contains", failures)
        self.assertIn("help_contains", failures)

    def test_rejects_emit_if_present_data_as_required(self) -> None:
        failures = self.failures_for(
            {
                "code": "ASSIGN_TARGET_NOT_ASSIGNABLE",
                "message_contains": ["assignment", "target", "assignable"],
                "data_keys": ["target_kind"],
            }
        )

        self.assertEqual(failures, ["data_keys"])

    def test_rejects_missing_one_of_group(self) -> None:
        failures = self.failures_for(
            {
                "code": "EFFECT_VIOLATION",
                "message_contains": [],
                "data_keys": [],
            }
        )

        self.assertEqual(failures, ["message_contains_any"])

    def test_rejects_missing_required_conditional_selector_value(self) -> None:
        failures = self.failures_for(
            {
                "code": "CALL_ARG_TYPE_MISMATCH",
                "message_contains": ["call", "argument", "type", "mismatch"],
                "data_keys": ["reason", "actual_type"],
            }
        )

        self.assertEqual(failures, ["data_equals"])

    def test_accepts_inactive_nested_conditional_selector(self) -> None:
        failures = self.failures_for(
            {
                "code": "PACKAGE_MANIFEST_MISSING",
                "message_contains": ["package", "manifest", "missing", "Eglur.toml"],
                "data_keys": ["package_role"],
                "data_equals": {"package_role": "root"},
            }
        )

        self.assertEqual(failures, [])

    def test_rejects_non_diagnostic_scalar_data_equals(self) -> None:
        diagnostic: dict[str, diagnostic_contracts.YamlValue] = {
            "code": "EFFECT_SIGNATURE",
            "message_contains": ["&mut Self"],
            "data_keys": ["value"],
            "data_equals": {"value": None},
        }

        with self.assertRaisesRegex(ValueError, "must be Str, i64, or Bool"):
            self.failures_for(diagnostic)

    def test_unknown_code_is_not_silently_dropped(self) -> None:
        failures = self.failures_for(
            {
                "code": "NOT_A_REAL_CODE",
                "message_contains": [],
                "data_keys": [],
            }
        )

        self.assertEqual(failures, ["code"])

    def test_wrong_channel_is_recorded_as_contract_failure(self) -> None:
        failures = self.failures_for(
            {
                "code": "RUNTIME_ABORT",
                "message_contains": ["runtime", "abort"],
                "data_keys": [],
            }
        )

        self.assertIn("channel", failures)

    def test_test_runner_runtime_code_is_not_silently_dropped(self) -> None:
        asserted, mentioned, failures = self.runtime_code_result_for("RUNTIME_ABORT")
        self.assertEqual(asserted, set())
        self.assertEqual(mentioned, set())
        self.assertEqual(failures, [])
        _, _, unknown_failures = self.runtime_code_result_for("NOT_A_REAL_CODE")
        self.assertIn(
            "code",
            unknown_failures,
        )
        _, _, wrong_channel_failures = self.runtime_code_result_for("EFFECT_VIOLATION")
        self.assertIn(
            "channel",
            wrong_channel_failures,
        )

    def test_current_repository_has_no_contract_failures(self) -> None:
        report = traceability_repository.build_report(REPO_ROOT)
        generated = traceability_codec.encode_report(report)
        lines = generated.decode().splitlines()

        self.assertEqual(report.contract_failures, ())
        self.assertEqual(
            generated,
            (REPO_ROOT / "tests/conformance/traceability.yaml").read_bytes(),
        )
        self.assertIn(
            f"release_history_root: {report.release_history_root}",
            lines,
        )
        self.assertIn(f"release: {report.release.text}", lines)
        self.assertIn("  required_code_count: 272", lines)
        self.assertIn("  contract_failure_count: 0", lines)

    def test_missing_manifest_suite_fails_for_required_and_optional_files(
        self,
    ) -> None:
        original_is_file = Path.is_file
        for relative_path in (
            "core/lexer.yaml",
            "optional/emit_if_present.yaml",
        ):
            missing = REPO_ROOT / "tests/conformance" / relative_path

            def is_file(path: Path, *, target: Path = missing) -> bool:
                return path != target and original_is_file(path)

            with (
                self.subTest(relative_path=relative_path),
                patch.object(Path, "is_file", is_file),
                self.assertRaisesRegex(
                    ValueError,
                    f"manifest suite file is missing: tests/conformance/{relative_path}",
                ),
            ):
                traceability_repository.build_report(REPO_ROOT)

    def test_hidden_severity_cannot_bind_diagnostic_metadata(self) -> None:
        source = """\
<!--
### `HIDDEN_CODE`
Severity: `error`
-->
### `VISIBLE_CODE`
```
Severity: `error`
```
Severity: `warning`
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "codes.md"
            path.write_text(source, encoding="utf-8")

            metadata = traceability_analysis.extract_code_metadata(
                str(path),
                visible_markdown_lines(path),
            )

        self.assertEqual(set(metadata), {"VISIBLE_CODE"})
        self.assertEqual(metadata["VISIBLE_CODE"].severity, "warning")


if __name__ == "__main__":
    unittest.main()

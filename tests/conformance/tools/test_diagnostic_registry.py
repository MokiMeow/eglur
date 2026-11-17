"""Regression tests for diagnostic profile composition."""

import tempfile
import unittest
from pathlib import Path
from shutil import copyfile
from typing import ClassVar

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


from eglur_conformance.diagnostics import catalog as diagnostic_catalog
from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.diagnostics.registry import load_diagnostic_registry
from eglur_conformance.suite import schema as conformance_schema


class DiagnosticRegistryTests(unittest.TestCase):
    """Exercise independent channel and coverage profiles."""

    registry: ClassVar[diagnostic_model.DiagnosticRegistry]

    @classmethod
    def setUpClass(cls) -> None:
        """Build the canonical registry once."""
        contracts = diagnostic_contracts.load_contracts(
            REPO_ROOT / diagnostic_contracts.DEFAULT_CONTRACT_PATH
        )
        cls.registry = load_diagnostic_registry(
            REPO_ROOT / "docs/spec/errors",
            contracts,
        )

    def test_registry_covers_every_diagnostic_contract(self) -> None:
        contracts = diagnostic_contracts.load_contracts(
            REPO_ROOT / diagnostic_contracts.DEFAULT_CONTRACT_PATH
        )
        self.assertEqual(set(self.registry), set(contracts))
        self.assertTrue(
            all(
                entry.channels.allowed_channels <= diagnostic_model.CHANNELS
                and entry.coverage.policy in diagnostic_model.COVERAGE_POLICIES
                for entry in self.registry.values()
            )
        )

    def test_registry_preserves_narrow_channel_classes(self) -> None:
        self.assertEqual(
            self.registry["EFFECT_VIOLATION"].channels.allowed_channels,
            frozenset({diagnostic_model.COMPILE_ERROR}),
        )
        self.assertEqual(
            self.registry["INT_OVERFLOW"].channels.allowed_channels,
            frozenset(
                {
                    diagnostic_model.COMPILE_ERROR,
                    diagnostic_model.VALUE_DIAGNOSTICS,
                }
            ),
        )
        self.assertEqual(
            self.registry["DETERMINISM_UNAVAILABLE"].channels.allowed_channels,
            frozenset({diagnostic_model.VALUE_DIAGNOSTICS}),
        )
        self.assertEqual(
            self.registry["RUNTIME_ABORT"].channels.allowed_channels,
            frozenset({diagnostic_model.RUNTIME_ERROR}),
        )
        self.assertEqual(
            self.registry["EXTERN_LINKAGE_FAILED"].channels.allowed_channels,
            frozenset(
                {
                    diagnostic_model.COMPILE_ERROR,
                    diagnostic_model.RUNTIME_ERROR,
                }
            ),
        )

    def test_registry_composes_independent_coverage_obligations(self) -> None:
        self.assertEqual(
            self.registry["EFFECT_VIOLATION"].coverage.policy,
            diagnostic_model.HARD_REQUIRED_ASSERTED,
        )
        self.assertEqual(
            self.registry["AXIS_NAMES_DROPPED"].coverage.policy,
            diagnostic_model.OPTIONAL_EMIT_IF_PRESENT,
        )
        self.assertEqual(
            self.registry["OWNERSHIP_PROFILE_DISABLED"].coverage.policy,
            diagnostic_model.EXCLUDED_NONBASELINE,
        )
        self.assertEqual(
            self.registry["DETERMINISM_UNAVAILABLE"].coverage.policy,
            diagnostic_model.CONDITIONAL_REQUIRED,
        )
        self.assertEqual(
            self.registry["DETERMINISM_UNAVAILABLE"].coverage.precondition,
            "strict_unavailable_path",
        )

    def test_schema_rejects_unknown_code(self) -> None:
        problems = conformance_schema.validate_diagnostic_assertion(
            {"code": "NOT_A_REAL_CODE"},
            "diagnostic",
            channel=diagnostic_model.COMPILE_ERROR,
            channel_registry=self.registry,
        )

        self.assertTrue(
            any("unknown diagnostic code" in item.message for item in problems)
        )

    def test_schema_rejects_wrong_channel(self) -> None:
        problems = conformance_schema.validate_diagnostic_assertion(
            {"code": "RUNTIME_ABORT"},
            "diagnostic",
            channel=diagnostic_model.COMPILE_ERROR,
            channel_registry=self.registry,
        )

        self.assertTrue(
            any("not allowed in compile_error" in item.message for item in problems)
        )

    def test_test_runner_runtime_code_uses_registry(self) -> None:
        base = {
            "module": "abort_failure",
            "name": "aborts!",
            "kind": "runtime_abort",
            "message_contains": ["abort"],
        }
        accepted = conformance_schema.validate_test_failure(
            {**base, "runtime_code": "RUNTIME_ABORT"},
            "failure",
            channel_registry=self.registry,
        )
        unknown = conformance_schema.validate_test_failure(
            {**base, "runtime_code": "NOT_A_REAL_CODE"},
            "failure",
            channel_registry=self.registry,
        )
        wrong_channel = conformance_schema.validate_test_failure(
            {**base, "runtime_code": "EFFECT_VIOLATION"},
            "failure",
            channel_registry=self.registry,
        )

        self.assertEqual(accepted, [])
        self.assertTrue(
            any("unknown diagnostic code" in item.message for item in unknown)
        )
        self.assertTrue(
            any(
                "not allowed in runtime_error" in item.message for item in wrong_channel
            )
        )

    def test_catalog_and_contract_drift_is_rejected(self) -> None:
        contracts = diagnostic_contracts.load_contracts(
            REPO_ROOT / diagnostic_contracts.DEFAULT_CONTRACT_PATH
        )
        contracts.pop("EFFECT_VIOLATION")

        with self.assertRaisesRegex(ValueError, "catalog/contract code mismatch"):
            load_diagnostic_registry(
                REPO_ROOT / "docs/spec/errors",
                contracts,
            )

    def test_duplicate_code_within_one_catalog_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "codes.md"
            path.write_text(
                "### `EFFECT_VIOLATION`\n### `EFFECT_VIOLATION`\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "duplicate diagnostic code"):
                diagnostic_catalog.extract_codes(path)

    def test_hidden_and_indented_catalog_examples_are_not_codes(self) -> None:
        source = """\
<!--
### `HIDDEN_COMMENT`
-->
```
### `HIDDEN_FENCE`
```
    ### `HIDDEN_INDENT`
### `VISIBLE_CODE`
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "codes.md"
            path.write_text(source, encoding="utf-8")

            codes = diagnostic_catalog.extract_codes(path)

        self.assertEqual(codes, {"VISIBLE_CODE"})

    def test_code_cannot_appear_in_multiple_catalogs(self) -> None:
        contracts = diagnostic_contracts.load_contracts(
            REPO_ROOT / diagnostic_contracts.DEFAULT_CONTRACT_PATH
        )
        with tempfile.TemporaryDirectory() as directory:
            errors_dir = Path(directory)
            for filename in (
                "compile-codes.md",
                "extension-codes.md",
                "runtime-codes.md",
            ):
                copyfile(
                    REPO_ROOT / "docs/spec/errors" / filename,
                    errors_dir / filename,
                )
            with (errors_dir / "runtime-codes.md").open(
                "a",
                encoding="utf-8",
            ) as stream:
                stream.write("\n### `EFFECT_VIOLATION`\n")

            with self.assertRaisesRegex(ValueError, "multiple catalogs"):
                load_diagnostic_registry(
                    errors_dir,
                    contracts,
                )


if __name__ == "__main__":
    unittest.main()

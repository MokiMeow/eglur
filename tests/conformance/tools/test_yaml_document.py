"""Regression tests for duplicate-key rejection at every YAML boundary."""

import tempfile
import unittest
from collections.abc import Callable
from pathlib import Path

from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.spec_release import artifact_inventory, open_items
from eglur_conformance.spec_release.signoff import codec as signoff_codec
from eglur_conformance.suite import schema as conformance_schema
from eglur_conformance.traceability import codec as traceability_codec
from eglur_conformance.yaml_document import load_yaml, load_yaml_text
from yaml.constructor import ConstructorError


class DuplicateKeyRejectionTests(unittest.TestCase):
    """Require duplicate keys to fail before semantic validation."""

    def assert_duplicate_rejected(
        self,
        source: str,
        loader: Callable[[Path], object],
    ) -> None:
        """Write one artifact and assert source-located duplicate rejection."""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact.yaml"
            path.write_text(source, encoding="utf-8")

            with self.assertRaises(ConstructorError) as caught:
                loader(path)

        message = str(caught.exception)
        self.assertIn("found duplicate key", message)
        self.assertIn(str(path), message)
        self.assertIn("line", message)

    def test_shared_loader_rejects_top_level_and_nested_duplicates(self) -> None:
        for source in (
            "status: fail\nstatus: pass\n",
            "outer:\n  verdict: NO-GO\n  verdict: GO\n",
        ):
            with self.subTest(source=source):
                self.assert_duplicate_rejected(source, load_yaml)

    def test_shared_text_loader_rejects_duplicates(self) -> None:
        with self.assertRaises(ConstructorError):
            load_yaml_text("release: v0.1.0\nrelease: v0.1.1\n")

    def test_manifest_loader_rejects_duplicates(self) -> None:
        self.assert_duplicate_rejected(
            "version: '0.1'\nversion: '9.9'\n",
            conformance_schema.parse_manifest,
        )

    def test_artifact_inventory_loader_rejects_duplicates(self) -> None:
        self.assert_duplicate_rejected(
            "status: draft\nstatus: released\n",
            lambda path: artifact_inventory.decode_inventory(
                load_yaml(path),
                str(path),
            ),
        )

    def test_suite_loader_rejects_nested_duplicates(self) -> None:
        self.assert_duplicate_rejected(
            "tests:\n- id: TEST-one\n  id: TEST-two\n",
            lambda path: conformance_schema.validate_suite_file(
                path,
                "synthetic.yaml",
            ),
        )

    def test_diagnostic_contract_loader_rejects_duplicates(self) -> None:
        self.assert_duplicate_rejected(
            "version: '0.1'\nversion: '9.9'\n",
            diagnostic_contracts.load_contracts,
        )

    def test_open_item_ledger_loader_rejects_duplicates(self) -> None:
        self.assert_duplicate_rejected(
            "status: draft\nstatus: released\n",
            lambda path: open_items.decode_ledger(load_yaml(path), str(path)),
        )

    def test_traceability_loader_rejects_duplicates(self) -> None:
        self.assert_duplicate_rejected(
            "version: '0.1'\nversion: '9.9'\n",
            lambda path: traceability_codec.decode_report(load_yaml(path)),
        )

    def test_spec_release_signoff_loader_rejects_duplicates(self) -> None:
        self.assert_duplicate_rejected(
            "verdict: NO-GO\nverdict: GO\n",
            lambda path: signoff_codec.decode_spec_package_signoff(load_yaml(path)),
        )


if __name__ == "__main__":
    unittest.main()

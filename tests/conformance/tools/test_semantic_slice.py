"""Contract tests for the generated first semantic fixture corpus."""

import json
import subprocess
import sys
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path
from typing import ClassVar
from unittest import mock

import yaml
from eglur_conformance.semantic_slice import codec, model, projection, repository
from eglur_conformance.suite import model as suite_model
from eglur_conformance.suite import repository as suite_repository

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


def fixture(
    corpus: model.SemanticFixtureCorpus,
    suite_id: str,
    test_id: str,
) -> model.SemanticFixture:
    """Return one named fixture from the checked projection."""
    identity = suite_model.TestIdentity(suite_id, test_id)
    return next(item for item in corpus.fixtures if item.identity == identity)


class SemanticSliceCorpusTests(unittest.TestCase):
    """Verify the checked-in selection and generated projection."""

    corpus: ClassVar[model.SemanticFixtureCorpus]
    artifact_bytes: ClassVar[bytes]

    @classmethod
    def setUpClass(cls) -> None:
        cls.corpus = repository.build_corpus(REPO_ROOT)
        cls.artifact_bytes = (REPO_ROOT / repository.CORPUS_PATH).read_bytes()

    def test_exact_selection_projects_once_in_canonical_order(self) -> None:
        identities = tuple(item.identity for item in self.corpus.fixtures)

        self.assertEqual(len(identities), model.CORE_V1_FIXTURE_COUNT)
        self.assertEqual(identities, tuple(sorted(identities)))
        self.assertEqual(len(set(identities)), len(identities))

    def test_multifile_invocation_preserves_entry_root_and_sources(self) -> None:
        expected = {
            (
                "core-effects",
                "EFF-imported-localizable-state-summary-allows-non-bang-local-call",
            ): ("app/main.eglur", "app/state.eglur"),
            (
                "core-effects",
                "EFF-imported-transitive-io-summary-rejects-non-bang-local-call",
            ): ("app/main.eglur", "app/state.eglur"),
            (
                "core-traits",
                "TRT-trait-method-call-works-when-trait-is-imported-at-call-site",
            ): ("app/main.eglur", "app/render.eglur"),
        }
        multifile = tuple(
            item
            for item in self.corpus.fixtures
            if isinstance(item.invocation.source, suite_model.FileSetSource)
        )

        self.assertEqual(len(multifile), len(expected))
        for selected in multifile:
            with self.subTest(identity=selected.identity):
                source = selected.invocation.source
                self.assertIsInstance(source, suite_model.FileSetSource)
                if not isinstance(source, suite_model.FileSetSource):
                    self.fail("multi-file fixture did not retain its file-set source")
                self.assertEqual(source.entry, "app/main.eglur")
                self.assertEqual(source.root, ".")
                self.assertEqual(
                    tuple(item.path for item in source.files),
                    expected[(selected.identity.suite_id, selected.identity.test_id)],
                )
                self.assertEqual(source.symlinks, ())
                self.assertEqual(selected.invocation.mode, "auto")
                self.assertEqual(selected.invocation.runner, "main")

    def test_marker_and_secondary_expectations_are_not_discarded(self) -> None:
        markers = {
            item.identity: item.expectation.outcome.diagnostic.primary_span
            for item in self.corpus.fixtures
            if isinstance(item.expectation.outcome, suite_model.CompileErrorOutcome)
            and isinstance(
                item.expectation.outcome.diagnostic.primary_span,
                suite_model.MarkerPrimarySpan,
            )
        }
        self.assertEqual(
            markers,
            {
                suite_model.TestIdentity(
                    "core-ownership",
                    "OWN-repeated-consuming-move-on-same-root-reports-ownership-double-move",
                ): suite_model.MarkerPrimarySpan("repeated-move", "code", 4),
                suite_model.TestIdentity(
                    "core-typecheck",
                    "TYC-later-local-declaration-is-not-hoisted",
                ): suite_model.MarkerPrimarySpan(
                    "use-before-local-declaration",
                    "code",
                    2,
                ),
                suite_model.TestIdentity(
                    "core-typecheck",
                    "TYC-redeclaration-precedes-invalid-initializer",
                ): suite_model.MarkerPrimarySpan(
                    "redeclaration-before-initializer",
                    "code",
                    3,
                ),
            },
        )

        exited = fixture(
            self.corpus,
            "core-program",
            "PRG-ok-entry-maps-to-zero-and-preserves-value",
        )
        self.assertEqual(exited.expectation.process_exit, 0)

        diagnostic_value = fixture(
            self.corpus,
            "core-numeric",
            "NUM-checked-dynamic-overflow-returns-numericerror",
        )
        self.assertEqual(
            tuple(item.code for item in diagnostic_value.expectation.value_diagnostics),
            ("INT_OVERFLOW",),
        )

    def test_checked_in_artifact_is_current_and_round_trips_exactly(self) -> None:
        decoded = codec.decode_corpus_bytes(self.artifact_bytes)

        self.assertEqual(decoded, self.corpus)
        self.assertEqual(codec.encode_corpus(decoded), self.artifact_bytes)
        self.assertTrue(repository.corpus_is_current(REPO_ROOT))


class SemanticSliceBoundaryTests(unittest.TestCase):
    """Reject ambiguous selections and unsupported first-slice fixtures."""

    def test_selection_rejects_duplicate_unknown_and_unsorted_identities(self) -> None:
        duplicate = b"version: '0.1'\nfixtures:\n- suite/TEST-a\n- suite/TEST-a\n"
        with self.assertRaisesRegex(ValueError, "unique"):
            codec.decode_selection_bytes(duplicate)

        duplicate_key = b"version: '0.1'\nversion: '0.1'\nfixtures: []\n"
        with self.assertRaisesRegex(yaml.YAMLError, "duplicate key"):
            codec.decode_selection_bytes(duplicate_key)

        malformed = b"version: '0.1'\nfixtures:\n- missing-separator\n"
        with self.assertRaisesRegex(ValueError, "suite/test"):
            codec.decode_selection_bytes(malformed)

        unsorted = b"version: '0.1'\nfixtures:\n- suite/TEST-z\n- suite/TEST-a\n"
        with self.assertRaisesRegex(ValueError, "sorted"):
            codec.decode_selection_bytes(unsorted)

        unknown_field = b"version: '0.1'\nfixtures: []\nunknown: true\n"
        with self.assertRaisesRegex(ValueError, "unsupported"):
            codec.decode_selection_bytes(unknown_field)

    def test_projection_rejects_a_missing_canonical_identity(self) -> None:
        inputs = suite_repository.load_conformance_inputs(REPO_ROOT)
        selection = codec.decode_selection_bytes(
            (REPO_ROOT / repository.SELECTION_PATH).read_bytes()
        )
        missing = suite_model.TestIdentity("core-program", "PRG-does-not-exist")
        changed = model.FixtureSelection(
            schema_version=selection.schema_version,
            identities=tuple(sorted((*selection.identities, missing))),
        )

        with self.assertRaisesRegex(ValueError, "does not resolve"):
            projection.project_corpus(inputs.bundle, changed, "sha256:" + "0" * 64)

    def test_model_rejects_registry_and_nonselected_outcome_families(self) -> None:
        corpus = repository.build_corpus(REPO_ROOT)
        selected = corpus.fixtures[0]
        acquisition = suite_model.RegistryAcquisition(
            package="registry.example/pkg",
            version="1.0.0",
            provider_steps=(suite_model.RegistryFinishStep("missing_release"),),
        )
        with self.assertRaisesRegex(ValueError, "registry acquisition"):
            replace(
                selected,
                invocation=replace(
                    selected.invocation,
                    registry_acquisitions=(acquisition,),
                ),
            )

        runtime = suite_model.RuntimeErrorOutcome(
            suite_model.DiagnosticAssertion(
                code="RUNTIME_ABORT",
                message_all_of=(),
                message_any_of=(),
                data_keys=(),
                data_equals=(),
                help_contains=None,
                related_contains=None,
                primary_span=None,
            )
        )
        with self.assertRaisesRegex(TypeError, "value and compile-error"):
            replace(
                selected,
                expectation=replace(selected.expectation, outcome=runtime),
            )

    def test_corpus_codec_rejects_duplicate_keys_and_unknown_fields(self) -> None:
        duplicate = b'{"schema_version":"0.1","schema_version":"0.1"}'
        with self.assertRaisesRegex(ValueError, "repeats key"):
            codec.decode_corpus_bytes(duplicate)

        corpus = repository.build_corpus(REPO_ROOT)
        payload = codec.corpus_payload(corpus)
        payload["unknown"] = True
        with self.assertRaisesRegex(ValueError, "unsupported"):
            codec.decode_corpus_bytes(json.dumps(payload).encode())

        payload = codec.corpus_payload(corpus)
        fixtures = payload["fixtures"]
        self.assertIsInstance(fixtures, list)
        fixtures.pop()
        with self.assertRaisesRegex(ValueError, "exactly 23 fixtures"):
            codec.decode_corpus_bytes(json.dumps(payload).encode())


class SemanticSlicePersistenceTests(unittest.TestCase):
    """Verify exact freshness checks and atomic artifact replacement."""

    def test_current_check_handles_missing_and_stale_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / repository.CORPUS_PATH
            with mock.patch.object(
                repository,
                "generated_corpus_bytes",
                return_value=b"current\n",
            ):
                self.assertFalse(repository.corpus_is_current(root))
                output.parent.mkdir(parents=True)
                output.write_bytes(b"stale\n")
                self.assertFalse(repository.corpus_is_current(root))
                output.write_bytes(b"current\n")
                self.assertTrue(repository.corpus_is_current(root))

    def test_missing_authored_selection_fails_closed(self) -> None:
        inputs = suite_repository.load_conformance_inputs(REPO_ROOT)
        with (
            tempfile.TemporaryDirectory() as directory,
            mock.patch.object(
                suite_repository,
                "load_conformance_inputs",
                return_value=inputs,
            ),
            self.assertRaises(FileNotFoundError),
        ):
            repository.build_corpus(Path(directory))

    def test_selection_byte_change_makes_the_generated_corpus_stale(self) -> None:
        inputs = suite_repository.load_conformance_inputs(REPO_ROOT)
        selection_bytes = (REPO_ROOT / repository.SELECTION_PATH).read_bytes()
        with (
            tempfile.TemporaryDirectory() as directory,
            mock.patch.object(
                suite_repository,
                "load_conformance_inputs",
                return_value=inputs,
            ),
        ):
            root = Path(directory)
            selection_path = root / repository.SELECTION_PATH
            selection_path.parent.mkdir(parents=True)
            selection_path.write_bytes(selection_bytes)
            corpus = repository.build_corpus(root)
            output = root / repository.CORPUS_PATH
            output.write_bytes(codec.encode_corpus(corpus))
            self.assertTrue(repository.corpus_is_current(root))

            selection_path.write_bytes(selection_bytes + b"# provenance change\n")
            self.assertFalse(repository.corpus_is_current(root))

    def test_write_is_atomic_and_removes_temporary_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(
                repository,
                "generated_corpus_bytes",
                return_value=b"generated\n",
            ):
                repository.write_corpus(root)

            output = root / repository.CORPUS_PATH
            self.assertEqual(output.read_bytes(), b"generated\n")
            self.assertEqual(tuple(output.parent.glob(f".{output.name}.*.tmp")), ())


class SemanticSliceCliTests(unittest.TestCase):
    """Exercise the supported command boundary outside the checkout."""

    def test_check_uses_the_explicit_repository_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            completed = subprocess.run(
                [
                    sys.executable,
                    str(TOOLS_DIR / "project_semantic_expectations.py"),
                    "--repo-root",
                    str(REPO_ROOT),
                    "--check",
                ],
                cwd=directory,
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("freshness check passed", completed.stdout)


if __name__ == "__main__":
    unittest.main()

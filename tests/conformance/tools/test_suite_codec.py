"""Regression tests for canonical conformance suite decoding."""

import unittest
from collections import Counter
from copy import deepcopy
from pathlib import Path
from typing import ClassVar, cast

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import model as suite_model
from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.yaml_document import YamlValue, load_yaml


class RepositoryBundleTests(unittest.TestCase):
    """Require canonical decoding to preserve the complete repository bundle."""

    manifest_value: ClassVar[YamlValue]
    suite_documents: ClassVar[dict[str, YamlValue]]
    bundle: ClassVar[suite_model.ConformanceBundle]

    @classmethod
    def setUpClass(cls) -> None:
        """Decode the checked-in bundle once for model coverage assertions."""
        manifest_path = REPO_ROOT / suite_schema.DEFAULT_MANIFEST_PATH
        cls.manifest_value = load_yaml(manifest_path)
        manifest = suite_schema.parse_manifest_value(
            cls.manifest_value,
            str(manifest_path),
        )
        cls.suite_documents = {
            entry.file: load_yaml(REPO_ROOT / manifest.root / entry.file)
            for entry in manifest.suites
        }
        cls.bundle = suite_codec.decode_bundle(
            cls.manifest_value,
            cls.suite_documents,
        )

    def test_decodes_every_suite_test_source_and_outcome_variant(self) -> None:
        """Assert exact inventory counts across all closed variant families."""
        tests = [test.case for test in self.bundle.tests]

        self.assertEqual(len(self.bundle.suites), 47)
        self.assertEqual(len(tests), 1944)
        self.assertEqual(
            Counter(
                type(suite.entry.participation).__name__ for suite in self.bundle.suites
            ),
            {
                "BaselineParticipation": 39,
                "ConditionalParticipation": 5,
                "OptionalParticipation": 3,
            },
        )
        self.assertEqual(
            Counter(type(test.invocation.source).__name__ for test in tests),
            {
                "Utf8Source": 1759,
                "FileSetSource": 184,
                "RawBytesSource": 1,
            },
        )
        self.assertEqual(
            Counter(type(test.expectation.outcome).__name__ for test in tests),
            {
                "ValueOutcome": 965,
                "CompileErrorOutcome": 966,
                "RuntimeErrorOutcome": 9,
                "TestRunOutcome": 4,
            },
        )

    def test_bundle_requires_the_exact_manifest_owned_document_set(self) -> None:
        """Reject both missing and unbound suite documents."""
        missing = dict(self.suite_documents)
        missing.pop(next(iter(missing)))
        with self.assertRaisesRegex(ValueError, "do not match manifest exactly"):
            suite_codec.decode_bundle(self.manifest_value, missing)

        unbound = dict(self.suite_documents)
        unbound["unbound.yaml"] = {"tests": []}
        with self.assertRaisesRegex(ValueError, "do not match manifest exactly"):
            suite_codec.decode_bundle(self.manifest_value, unbound)

    def test_manifest_rejects_optional_suites_with_preconditions(self) -> None:
        """Keep optional and conditionally required participation disjoint."""
        manifest = load_yaml(REPO_ROOT / suite_schema.DEFAULT_MANIFEST_PATH)
        self.assertIsInstance(manifest, dict)
        suites = manifest["suites"]
        self.assertIsInstance(suites, list)
        first_suite = suites[0]
        self.assertIsInstance(first_suite, dict)
        first_suite["required"] = False
        first_suite["precondition"] = "ffi_linkage_probe"

        with self.assertRaisesRegex(
            ValueError,
            "precondition is invalid when required is false",
        ):
            suite_schema.parse_manifest_value(manifest)

    def test_bundle_rejects_global_test_id_collisions(self) -> None:
        """Reject a test identity reused by two different suite files."""
        documents = deepcopy(self.suite_documents)
        first_suite, second_suite = self.bundle.suites[:2]
        first_test_id = first_suite.document.tests[0].test_id
        second_document = documents[second_suite.entry.file]
        self.assertIsInstance(second_document, dict)
        second_tests = second_document["tests"]
        self.assertIsInstance(second_tests, list)
        second_test = second_tests[0]
        self.assertIsInstance(second_test, dict)
        second_test["id"] = first_test_id

        with self.assertRaisesRegex(ValueError, "global duplicate test id"):
            suite_codec.decode_bundle(self.manifest_value, documents)

    def test_bundle_owns_manifest_qualified_test_lookup(self) -> None:
        """Resolve suite and test facts without rebuilding caller-side indexes."""
        suite = self.bundle.suites[0]
        case = suite.document.tests[0]
        identity = suite_model.TestIdentity(suite.entry.suite_id, case.test_id)

        self.assertIs(self.bundle.find_suite(identity.suite_id), suite)
        found = self.bundle.find_test(identity)
        self.assertIsNotNone(found)
        assert found is not None
        self.assertIs(found.suite, suite.entry)
        self.assertIs(found.case, case)
        self.assertEqual(found.identity, identity)

        unknown = suite_model.TestIdentity("unknown-suite", "unknown-test")
        self.assertIsNone(self.bundle.find_suite(unknown.suite_id))
        self.assertIsNone(self.bundle.find_test(unknown))
        unknown_test = suite_model.TestIdentity(suite.entry.suite_id, "unknown-test")
        self.assertIs(self.bundle.find_suite(unknown_test.suite_id), suite)
        self.assertIsNone(self.bundle.find_test(unknown_test))

        with self.assertRaisesRegex(TypeError, "string suite_id"):
            self.bundle.find_suite(cast(str, 1))
        with self.assertRaisesRegex(TypeError, "TestIdentity"):
            self.bundle.find_test(cast(suite_model.TestIdentity, ("suite", "test")))

    def test_identity_enforces_only_intrinsic_validity(self) -> None:
        """Keep local identity validity separate from bundle existence policy."""
        first = suite_model.TestIdentity("a-suite", "a-test")
        second = suite_model.TestIdentity("a-suite", "b-test")

        self.assertEqual(str(first), "a-suite/a-test")
        self.assertEqual(sorted((second, first)), [first, second])
        self.assertEqual(len({first, suite_model.TestIdentity("a-suite", "a-test")}), 1)
        self.assertIsNone(self.bundle.find_test(first))

        with self.assertRaisesRegex(TypeError, "suite_id"):
            suite_model.TestIdentity(cast(str, 1), "test")
        with self.assertRaisesRegex(ValueError, "suite_id"):
            suite_model.TestIdentity("", "test")
        with self.assertRaisesRegex(TypeError, "test_id"):
            suite_model.TestIdentity("suite", cast(str, 1))
        with self.assertRaisesRegex(ValueError, "test_id"):
            suite_model.TestIdentity("suite", "")
        with self.assertRaisesRegex(TypeError, "SuiteEntry"):
            suite_model.ConformanceTest(
                cast(suite_model.SuiteEntry, "suite"),
                self.bundle.suites[0].document.tests[0],
            )
        suite = self.bundle.suites[0]
        with self.assertRaisesRegex(ValueError, "suite_id"):
            suite_model.ConformanceTest(
                suite_model.SuiteEntry(
                    "",
                    suite.entry.file,
                    suite.entry.participation,
                ),
                suite.document.tests[0],
            )


class SuiteDocumentTests(unittest.TestCase):
    """Exercise source-level distinctions retained by the canonical model."""

    def test_preserves_optional_diagnostic_field_presence(self) -> None:
        """Distinguish an absent optional field from a present probe assertion."""
        document: YamlValue = {
            "tests": [
                {
                    "id": "SYN-001",
                    "category": "diagnostic",
                    "title": "absent rendering probes",
                    "code": "fn main() -> Unit {}",
                    "expect": {
                        "compile_error": {
                            "code": "SYNTHETIC_ERROR",
                            "message_contains": [],
                            "data_keys": [],
                        }
                    },
                },
                {
                    "id": "SYN-002",
                    "category": "diagnostic",
                    "title": "present rendering probes",
                    "code": "fn main() -> Unit {}",
                    "expect": {
                        "compile_error": {
                            "code": "SYNTHETIC_ERROR",
                            "message_contains": [],
                            "data_keys": [],
                            "help_contains": ["use a valid declaration"],
                            "related_contains": ["invalid declaration"],
                        }
                    },
                },
            ]
        }

        decoded = suite_codec.decode_suite_document(document, "synthetic.yaml")
        first = decoded.tests[0].expectation.outcome
        second = decoded.tests[1].expectation.outcome
        self.assertIsInstance(first, suite_model.CompileErrorOutcome)
        self.assertIsInstance(second, suite_model.CompileErrorOutcome)
        if not isinstance(first, suite_model.CompileErrorOutcome):
            self.fail("first outcome was not narrowed")
        if not isinstance(second, suite_model.CompileErrorOutcome):
            self.fail("second outcome was not narrowed")

        self.assertIsNone(first.diagnostic.help_contains)
        self.assertIsNone(first.diagnostic.related_contains)
        self.assertEqual(
            second.diagnostic.help_contains,
            ("use a valid declaration",),
        )
        self.assertEqual(
            second.diagnostic.related_contains,
            ("invalid declaration",),
        )

    def test_decodes_symlink_topology_in_canonical_path_order(self) -> None:
        invocation = suite_codec.decode_fixture_invocation(
            {
                "files": {
                    "src/main.eglur": "fn main() -> Unit = ()\n",
                },
                "symlinks": {
                    "links/z": "../target-z",
                    "links/a": "./target-a",
                },
                "entry": "src/main.eglur",
            }
        )

        self.assertIsInstance(invocation.source, suite_model.FileSetSource)
        if not isinstance(invocation.source, suite_model.FileSetSource):
            self.fail("source was not narrowed")
        self.assertEqual(
            invocation.source.symlinks,
            (
                suite_model.FixtureSymlink("links/a", "./target-a"),
                suite_model.FixtureSymlink("links/z", "../target-z"),
            ),
        )

        without_topology = suite_codec.decode_fixture_invocation(
            {
                "files": {
                    "src/main.eglur": "fn main() -> Unit = ()\n",
                },
                "entry": "src/main.eglur",
            }
        )
        self.assertIsInstance(without_topology.source, suite_model.FileSetSource)
        if not isinstance(without_topology.source, suite_model.FileSetSource):
            self.fail("source was not narrowed")
        self.assertEqual(without_topology.source.symlinks, ())

    def test_decodes_registry_acquisitions_without_erasing_provider_chronology(
        self,
    ) -> None:
        """Preserve raw entry variants, repetition, and ordered finish."""
        invocation = suite_codec.decode_fixture_invocation(
            {
                "files": {
                    "Eglur.toml": '[package]\nname = "app"\nversion = "0.1.0"\n',
                    "src/main.eglur": "fn main!() -> Unit = ()\n",
                },
                "entry": "src/main.eglur",
                "registry_acquisitions": [
                    {
                        "package": "packages.example.com/acme/zeta",
                        "version": "2.0.0",
                        "provider_steps": [
                            {
                                "emit": {
                                    "kind": "file",
                                    "path": "src/Z.eglur",
                                    "text": "z",
                                }
                            },
                            {"finish": "complete"},
                        ],
                    },
                    {
                        "package": "packages.example.com/acme/alpha",
                        "version": "1.0.0",
                        "provider_steps": [
                            {
                                "emit": {
                                    "kind": "file",
                                    "path": "src/dup.eglur",
                                    "text": "z",
                                }
                            },
                            {
                                "repeat": {
                                    "count": 2,
                                    "emit": {
                                        "kind": "fifo",
                                        "path_repeat": {
                                            "byte_hex": "ff",
                                            "count": 3,
                                        },
                                    },
                                }
                            },
                            {"emit": {"kind": "directory", "path": "src"}},
                            {
                                "emit": {
                                    "kind": "symlink",
                                    "path": "src/alias",
                                    "target": "A.eglur",
                                }
                            },
                            {
                                "emit": {
                                    "kind": "file",
                                    "path": "src/A.eglur",
                                    "text": "a",
                                }
                            },
                            {
                                "emit": {
                                    "kind": "file",
                                    "path": "src/dup.eglur",
                                    "content_hex": "00",
                                }
                            },
                            {"finish": "complete"},
                        ],
                    },
                ],
            }
        )

        self.assertEqual(
            tuple(
                acquisition.package for acquisition in invocation.registry_acquisitions
            ),
            (
                "packages.example.com/acme/alpha",
                "packages.example.com/acme/zeta",
            ),
        )
        self.assertEqual(
            invocation.registry_acquisitions[0].provider_steps,
            (
                suite_model.RegistryEmitStep(
                    suite_model.RegistryFileEntry(b"src/dup.eglur", b"z")
                ),
                suite_model.RegistryRepeatStep(
                    2,
                    suite_model.RegistrySpecialEntry(
                        "fifo",
                        suite_model.RegistryRepeatedByte(0xFF, 3),
                    ),
                ),
                suite_model.RegistryEmitStep(
                    suite_model.RegistryDirectoryEntry(b"src")
                ),
                suite_model.RegistryEmitStep(
                    suite_model.RegistryLinkEntry(
                        "symlink",
                        b"src/alias",
                        b"A.eglur",
                    )
                ),
                suite_model.RegistryEmitStep(
                    suite_model.RegistryFileEntry(b"src/A.eglur", b"a")
                ),
                suite_model.RegistryEmitStep(
                    suite_model.RegistryFileEntry(b"src/dup.eglur", b"\x00")
                ),
                suite_model.RegistryFinishStep("complete"),
            ),
        )

    def test_orders_registry_acquisitions_by_canonical_lexical_coordinate(
        self,
    ) -> None:
        invocation = suite_codec.decode_fixture_invocation(
            {
                "files": {
                    "Eglur.toml": '[package]\nname = "app"\nversion = "0.1.0"\n',
                    "src/main.eglur": "fn main!() -> Unit = ()\n",
                },
                "entry": "src/main.eglur",
                "registry_acquisitions": [
                    {
                        "package": "packages.example.com/acme/math",
                        "version": "2.0.0",
                        "provider_steps": [{"finish": "transport_failure"}],
                    },
                    {
                        "package": "packages.example.com/acme/math",
                        "version": "10.0.0",
                        "provider_steps": [{"finish": "missing_release"}],
                    },
                ],
            }
        )

        self.assertEqual(
            tuple(
                acquisition.version for acquisition in invocation.registry_acquisitions
            ),
            ("10.0.0", "2.0.0"),
        )
        self.assertEqual(
            invocation.registry_acquisitions[0].provider_steps,
            (suite_model.RegistryFinishStep("missing_release"),),
        )
        self.assertEqual(
            invocation.registry_acquisitions[1].provider_steps,
            (suite_model.RegistryFinishStep("transport_failure"),),
        )

    def test_registry_provider_models_reject_schema_impossible_states(self) -> None:
        entry = suite_model.RegistryDirectoryEntry(b"x")

        with self.assertRaisesRegex(ValueError, "byte must be in range"):
            suite_model.RegistryRepeatedByte(True, 1)
        with self.assertRaisesRegex(ValueError, "count must be a positive i64"):
            suite_model.RegistryRepeatedByte(0, True)
        with self.assertRaisesRegex(ValueError, "repeat count must be a positive i64"):
            suite_model.RegistryRepeatStep(True, entry)
        with self.assertRaisesRegex(ValueError, "non-empty tuple"):
            suite_model.RegistryAcquisition(
                "packages.example.com/acme/math",
                "1.2.3",
                (),
            )
        with self.assertRaisesRegex(TypeError, "end with a finish"):
            suite_model.RegistryAcquisition(
                "packages.example.com/acme/math",
                "1.2.3",
                (suite_model.RegistryEmitStep(entry),),
            )
        with self.assertRaisesRegex(ValueError, "cannot finish before"):
            suite_model.RegistryAcquisition(
                "packages.example.com/acme/math",
                "1.2.3",
                (
                    suite_model.RegistryFinishStep("complete"),
                    suite_model.RegistryFinishStep("complete"),
                ),
            )
        with self.assertRaisesRegex(ValueError, "cannot follow entry emissions"):
            suite_model.RegistryAcquisition(
                "packages.example.com/acme/math",
                "1.2.3",
                (
                    suite_model.RegistryEmitStep(entry),
                    suite_model.RegistryFinishStep("missing_release"),
                ),
            )


if __name__ == "__main__":
    unittest.main()

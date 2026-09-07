"""Regression tests for the registry-cache implementation probe contract."""

import unittest
from collections.abc import Callable
from copy import deepcopy
from dataclasses import replace
from pathlib import Path
from typing import ClassVar, cast

from eglur_conformance.registry_cache import codec, model, policy, repository
from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import model as suite_model
from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.yaml_document import YamlValue, load_yaml

_TOOLS_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _TOOLS_DIR.parents[2]
_PROFILE_PATH = _REPO_ROOT / repository.DEFAULT_PROFILE_PATH
_MANIFEST_PATH = _REPO_ROOT / suite_schema.DEFAULT_MANIFEST_PATH


def _load_profile_mapping() -> dict[str, YamlValue]:
    document = load_yaml(_PROFILE_PATH)
    if not isinstance(document, dict):
        raise TypeError("registry-cache profile must be a mapping")
    if not all(isinstance(key, str) for key in document):
        raise TypeError("registry-cache profile must use string keys")
    return deepcopy(cast(dict[str, YamlValue], document))


class RegistryCacheCodecTests(unittest.TestCase):
    """Keep the serialized profile closed and complete."""

    def load_mapping(self) -> dict[str, YamlValue]:
        """Return a mutable canonical profile mapping."""
        return _load_profile_mapping()

    def case_by_scenario(
        self,
        document: dict[str, YamlValue],
        scenario: str,
    ) -> dict[str, YamlValue]:
        """Return one mutable serialized case by scenario."""
        cases = document["cases"]
        self.assertIsInstance(cases, list)
        for case in cases:
            self.assertIsInstance(case, dict)
            if case.get("scenario") == scenario:
                return case
        self.fail(f"missing scenario {scenario}")

    def test_canonical_profile_covers_every_required_scenario(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH), str(_PROFILE_PATH))

        self.assertEqual(profile.required_capability, model.REQUIRED_CAPABILITY)
        self.assertEqual(profile.fixture_manifest, model.FIXTURE_MANIFEST_PATH)
        self.assertEqual(profile.observations, model.REGISTRY_CACHE_OBSERVATIONS)
        self.assertEqual(len(profile.cases), 9)
        self.assertEqual(
            tuple(case.case_id for case in profile.cases),
            tuple(sorted(case.case_id for case in profile.cases)),
        )
        self.assertEqual(
            {case.scenario for case in profile.cases},
            set(model.REQUIRED_SCENARIOS),
        )

    def test_rejects_unknown_keys_and_incomplete_observations(self) -> None:
        unknown_root = self.load_mapping()
        unknown_root["extra"] = True

        unknown_case = self.load_mapping()
        cases = unknown_case["cases"]
        self.assertIsInstance(cases, list)
        first_case = cases[0]
        self.assertIsInstance(first_case, dict)
        first_case["extra"] = True

        missing_observation = self.load_mapping()
        observations = missing_observation["observations"]
        self.assertIsInstance(observations, list)
        observations.pop()

        mutations = (
            (unknown_root, "unsupported"),
            (unknown_case, "unsupported"),
            (missing_observation, "must match the closed profile"),
        )
        for mutation, message in mutations:
            with (
                self.subTest(message=message),
                self.assertRaisesRegex(ValueError, message),
            ):
                codec.decode_profile(mutation)

    def test_rejects_duplicate_ids_and_missing_scenarios(self) -> None:
        duplicate = self.load_mapping()
        duplicate_cases = duplicate["cases"]
        self.assertIsInstance(duplicate_cases, list)
        first_case = duplicate_cases[0]
        second_case = duplicate_cases[1]
        self.assertIsInstance(first_case, dict)
        self.assertIsInstance(second_case, dict)
        second_case["id"] = first_case["id"]
        with self.assertRaisesRegex(ValueError, "duplicate ids"):
            codec.decode_profile(duplicate)

        missing = self.load_mapping()
        missing_cases = missing["cases"]
        self.assertIsInstance(missing_cases, list)
        missing_cases.pop()
        with self.assertRaisesRegex(ValueError, "missing required scenarios"):
            codec.decode_profile(missing)

        duplicate_scenario = self.load_mapping()
        duplicate_scenario_cases = duplicate_scenario["cases"]
        self.assertIsInstance(duplicate_scenario_cases, list)
        repeated_case = deepcopy(duplicate_scenario_cases[0])
        self.assertIsInstance(repeated_case, dict)
        repeated_case["id"] = "RCACHE-duplicate-scenario"
        duplicate_scenario_cases.append(repeated_case)
        with self.assertRaisesRegex(ValueError, "duplicate scenarios"):
            codec.decode_profile(duplicate_scenario)

    def test_concurrency_participant_count_is_exact_and_not_boolean(self) -> None:
        for participants, error_type, message in (
            (True, TypeError, "must be an integer"),
            (3, ValueError, "must be 2"),
        ):
            mutation = self.load_mapping()
            concurrent = self.case_by_scenario(
                mutation,
                "concurrent_same_coordinate_converges",
            )
            concurrent["participants"] = participants
            with self.subTest(participants=participants):
                try:
                    codec.decode_profile(mutation)
                except error_type as error:
                    self.assertRegex(str(error), message)
                    continue
                self.fail("invalid participant count was accepted")

        mutation = self.load_mapping()
        concurrent = self.case_by_scenario(
            mutation,
            "concurrent_same_coordinate_converges",
        )
        concurrent["publication_pause"] = "after_logical_commit"
        with self.assertRaisesRegex(ValueError, "publication_pause"):
            codec.decode_profile(mutation)

        mutation = self.load_mapping()
        concurrent = self.case_by_scenario(
            mutation,
            "concurrent_same_coordinate_converges",
        )
        concurrent["lookup_rendezvous"] = "after_publication"
        with self.assertRaisesRegex(ValueError, "lookup_rendezvous"):
            codec.decode_profile(mutation)

    def test_case_declaration_order_is_not_semantic(self) -> None:
        mutation = self.load_mapping()
        cases = mutation["cases"]
        self.assertIsInstance(cases, list)
        cases.reverse()

        profile = codec.decode_profile(mutation)

        self.assertEqual(
            tuple(case.case_id for case in profile.cases),
            tuple(sorted(case.case_id for case in profile.cases)),
        )

    def test_canonical_models_reject_noncanonical_programmatic_states(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        concurrent = next(
            case
            for case in profile.cases
            if isinstance(case, model.ConcurrentConvergenceCase)
        )

        with self.assertRaisesRegex(ValueError, "observations"):
            replace(profile, observations=())
        with self.assertRaisesRegex(ValueError, "participants"):
            replace(concurrent, participants=1)
        with self.assertRaisesRegex(TypeError, "participants"):
            replace(concurrent, participants=cast(int, 2.0))
        with self.assertRaisesRegex(TypeError, "TestIdentity"):
            replace(concurrent, fixture=cast(suite_model.TestIdentity, "fixture"))
        with self.assertRaisesRegex(TypeError, "suite_id"):
            suite_model.TestIdentity(cast(str, 1), "test")
        with self.assertRaisesRegex(TypeError, "cases"):
            replace(
                profile,
                cases=cast(tuple[model.ProbeCase, ...], list(profile.cases)),
            )


class RegistryCachePolicyTests(unittest.TestCase):
    """Bind probe cases to suitable manifest-owned source fixtures."""

    bundle: ClassVar[suite_model.ConformanceBundle]

    @classmethod
    def setUpClass(cls) -> None:
        """Decode the canonical source fixture bundle once."""
        manifest_value = load_yaml(_MANIFEST_PATH)
        manifest = suite_schema.parse_manifest_value(
            manifest_value,
            str(_MANIFEST_PATH),
        )
        suite_documents = {
            entry.file: load_yaml(_REPO_ROOT / manifest.root / entry.file)
            for entry in manifest.suites
        }
        cls.bundle = suite_codec.decode_bundle(manifest_value, suite_documents)

    def case_by_scenario(
        self,
        document: dict[str, YamlValue],
        scenario: str,
    ) -> dict[str, YamlValue]:
        """Return one mutable serialized case by scenario."""
        cases = document["cases"]
        self.assertIsInstance(cases, list)
        for case in cases:
            self.assertIsInstance(case, dict)
            if case.get("scenario") == scenario:
                return case
        self.fail(f"missing scenario {scenario}")

    def replace_compile_data(
        self,
        test_id: str,
        key: str,
        value: str,
    ) -> suite_model.ConformanceBundle:
        """Return a bundle with one compile diagnostic datum replaced."""
        updated_suites: list[suite_model.ConformanceSuite] = []
        found = False
        for suite in self.bundle.suites:
            updated_tests: list[suite_model.TestCase] = []
            for test in suite.document.tests:
                if test.test_id != test_id:
                    updated_tests.append(test)
                    continue
                outcome = test.expectation.outcome
                self.assertIsInstance(outcome, suite_model.CompileErrorOutcome)
                if not isinstance(outcome, suite_model.CompileErrorOutcome):
                    self.fail("fixture outcome was not narrowed")
                data_equals = dict(outcome.diagnostic.data_equals)
                data_equals[key] = value
                diagnostic = replace(
                    outcome.diagnostic,
                    data_equals=tuple(sorted(data_equals.items())),
                )
                expectation = replace(
                    test.expectation,
                    outcome=replace(outcome, diagnostic=diagnostic),
                )
                updated_tests.append(replace(test, expectation=expectation))
                found = True
            updated_suites.append(
                replace(
                    suite,
                    document=replace(
                        suite.document,
                        tests=tuple(updated_tests),
                    ),
                )
            )
        self.assertTrue(found)
        return replace(self.bundle, suites=tuple(updated_suites))

    def replace_test(
        self,
        test_id: str,
        transform: Callable[[suite_model.TestCase], suite_model.TestCase],
        *,
        bundle: suite_model.ConformanceBundle | None = None,
    ) -> suite_model.ConformanceBundle:
        """Return a bundle with one fixture transformed."""
        updated_suites: list[suite_model.ConformanceSuite] = []
        found = False
        source_bundle = self.bundle if bundle is None else bundle
        for suite in source_bundle.suites:
            updated_tests: list[suite_model.TestCase] = []
            for test in suite.document.tests:
                if test.test_id == test_id:
                    updated_tests.append(transform(test))
                    found = True
                else:
                    updated_tests.append(test)
            updated_suites.append(
                replace(
                    suite,
                    document=replace(
                        suite.document,
                        tests=tuple(updated_tests),
                    ),
                )
            )
        self.assertTrue(found)
        return replace(source_bundle, suites=tuple(updated_suites))

    def replace_provider_finish(
        self,
        test_id: str,
        outcome: suite_model.RegistryProviderFinish,
    ) -> suite_model.ConformanceBundle:
        """Return a bundle with one fixture's provider finish replaced."""

        def transform(test: suite_model.TestCase) -> suite_model.TestCase:
            acquisition = test.invocation.registry_acquisitions[0]
            finish = acquisition.provider_steps[-1]
            self.assertIsInstance(finish, suite_model.RegistryFinishStep)
            if not isinstance(finish, suite_model.RegistryFinishStep):
                self.fail("registry fixture did not end with a finish step")
            updated_acquisition = replace(
                acquisition,
                provider_steps=(
                    *acquisition.provider_steps[:-1],
                    replace(finish, outcome=outcome),
                ),
            )
            return replace(
                test,
                invocation=replace(
                    test.invocation,
                    registry_acquisitions=(updated_acquisition,),
                ),
            )

        return self.replace_test(test_id, transform)

    def replace_source_text(
        self,
        test_id: str,
        path: str,
        transform_text: Callable[[str], str],
        *,
        bundle: suite_model.ConformanceBundle | None = None,
    ) -> suite_model.ConformanceBundle:
        """Return a bundle with one source file transformed."""

        def transform(test: suite_model.TestCase) -> suite_model.TestCase:
            source = test.invocation.source
            self.assertIsInstance(source, suite_model.FileSetSource)
            if not isinstance(source, suite_model.FileSetSource):
                self.fail("fixture source was not narrowed")
            found = False
            files: list[suite_model.SourceFile] = []
            for source_file in source.files:
                if source_file.path == path:
                    files.append(
                        replace(source_file, text=transform_text(source_file.text))
                    )
                    found = True
                else:
                    files.append(source_file)
            self.assertTrue(found)
            return replace(
                test,
                invocation=replace(
                    test.invocation,
                    source=replace(source, files=tuple(files)),
                ),
            )

        return self.replace_test(test_id, transform, bundle=bundle)

    def remove_source_file(
        self,
        test_id: str,
        path: str,
    ) -> suite_model.ConformanceBundle:
        """Return a bundle without one source file."""

        def transform(test: suite_model.TestCase) -> suite_model.TestCase:
            source = test.invocation.source
            self.assertIsInstance(source, suite_model.FileSetSource)
            if not isinstance(source, suite_model.FileSetSource):
                self.fail("fixture source was not narrowed")
            files = tuple(item for item in source.files if item.path != path)
            self.assertEqual(len(files), len(source.files) - 1)
            return replace(
                test,
                invocation=replace(
                    test.invocation,
                    source=replace(source, files=files),
                ),
            )

        return self.replace_test(test_id, transform)

    def replace_provider_file_entry(
        self,
        test_id: str,
        path: bytes,
        transform_entry: Callable[
            [suite_model.RegistryFileEntry],
            suite_model.RegistryEntry,
        ],
    ) -> suite_model.ConformanceBundle:
        """Return a bundle with one provider file entry transformed."""

        def transform(test: suite_model.TestCase) -> suite_model.TestCase:
            acquisition = test.invocation.registry_acquisitions[0]
            found = False
            steps: list[suite_model.RegistryProviderStep] = []
            for step in acquisition.provider_steps:
                if (
                    isinstance(step, suite_model.RegistryEmitStep)
                    and isinstance(step.entry, suite_model.RegistryFileEntry)
                    and step.entry.path == path
                ):
                    steps.append(replace(step, entry=transform_entry(step.entry)))
                    found = True
                else:
                    steps.append(step)
            self.assertTrue(found)
            return replace(
                test,
                invocation=replace(
                    test.invocation,
                    registry_acquisitions=(
                        replace(acquisition, provider_steps=tuple(steps)),
                    ),
                ),
            )

        return self.replace_test(test_id, transform)

    def test_repository_profile_and_source_references_validate(self) -> None:
        self.assertEqual(
            repository.validate_repository(_REPO_ROOT, _MANIFEST_PATH),
            [],
        )

    def test_coordinate_isolation_changes_each_component_independently(self) -> None:
        for component in ("package", "version", "integrity"):
            with self.subTest(component=component):
                mutable = _load_profile_mapping()
                case = self.case_by_scenario(
                    mutable,
                    "complete_coordinate_isolation",
                )
                case[f"{component}_fixture"] = deepcopy(case["priming_fixture"])
                profile = codec.decode_profile(mutable)

                problems = policy.validate_profile(profile, self.bundle)

                self.assertTrue(
                    any(
                        f"must change only {component}" in problem.message
                        for problem in problems
                    )
                )

    def test_coordinate_isolation_preserves_noncoordinate_inputs(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        version_test = "MOD-registry-release-version-must-match-target-package-metadata"
        source_bundle = self.replace_source_text(
            version_test,
            "src/main.eglur",
            lambda text: text.replace("double(3)", "double(4)"),
        )

        def reorder_provider_steps(test: suite_model.TestCase) -> suite_model.TestCase:
            acquisition = test.invocation.registry_acquisitions[0]
            first, second, *remaining = acquisition.provider_steps
            return replace(
                test,
                invocation=replace(
                    test.invocation,
                    registry_acquisitions=(
                        replace(
                            acquisition,
                            provider_steps=(second, first, *remaining),
                        ),
                    ),
                ),
            )

        provider_bundle = self.replace_test(version_test, reorder_provider_steps)

        for bundle in (source_bundle, provider_bundle):
            with self.subTest(bundle=bundle):
                problems = policy.validate_profile(profile, bundle)
                self.assertTrue(
                    any(
                        "version fixture must preserve non-coordinate invocation inputs"
                        in problem.message
                        for problem in problems
                    ),
                    [problem.message for problem in problems],
                )

    def test_coordinate_isolation_binds_expected_and_actual_digests(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        isolated_test = "MOD-registry-canonical-tree-integrity-mismatch-is-rejected"
        cases = (
            (
                "expected",
                "tree-sha256:1111111111111111111111111111111111111111111111111111111111111111",
                "expected digest must match its lock",
            ),
            (
                "actual",
                "tree-sha256:2222222222222222222222222222222222222222222222222222222222222222",
                "actual digest must match the priming snapshot",
            ),
        )
        for key, value, message in cases:
            with self.subTest(key=key):
                bundle = self.replace_compile_data(isolated_test, key, value)
                problems = policy.validate_profile(profile, bundle)

                self.assertTrue(any(message in problem.message for problem in problems))

    def test_provider_finish_controls_partial_entry_interpretation(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        success_test = "MOD-qualified-registry-dependency-resolves-from-canonical-tree"
        transport_test = "MOD-registry-transport-failure-is-a-closed-outcome"

        success_bundle = self.replace_provider_finish(
            success_test,
            "transport_failure",
        )
        failure_bundle = self.replace_provider_finish(transport_test, "complete")

        success_problems = policy.validate_profile(profile, success_bundle)
        failure_problems = policy.validate_profile(profile, failure_bundle)

        self.assertTrue(
            any(
                "complete provider acquisition" in item.message
                for item in success_problems
            )
        )
        self.assertTrue(
            any("valid UTF-8 path bytes" in item.message for item in failure_problems)
        )

    def test_registry_diagnostics_bind_the_selected_target(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        integrity_test = "MOD-registry-canonical-tree-integrity-mismatch-is-rejected"
        transport_test = "MOD-registry-transport-failure-is-a-closed-outcome"
        invalid_test = (
            "MOD-registry-valid-tree-without-manifest-retains-package-diagnostic"
        )

        mutations = (
            (
                self.replace_compile_data(
                    integrity_test,
                    "package",
                    "packages.example.com/other/math",
                ),
                "diagnostic package must match",
            ),
            (
                self.replace_compile_data(transport_test, "version", "9.9.9"),
                "diagnostic version must match",
            ),
            (
                self.replace_compile_data(invalid_test, "package_role", "root"),
                "target-package diagnostic package_role",
            ),
            (
                self.replace_compile_data(
                    invalid_test,
                    "registry_package",
                    "packages.example.com/other/math",
                ),
                "target-package diagnostic registry_package",
            ),
        )
        for bundle, message in mutations:
            with self.subTest(message=message):
                problems = policy.validate_profile(profile, bundle)
                self.assertTrue(
                    any(message in problem.message for problem in problems),
                    [problem.message for problem in problems],
                )

    def test_fixture_selection_binds_manifest_lock_and_diagnostic_dependency(
        self,
    ) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        success_test = "MOD-qualified-registry-dependency-resolves-from-canonical-tree"
        invalid_test = (
            "MOD-registry-valid-tree-without-manifest-retains-package-diagnostic"
        )
        mutations = (
            (
                self.replace_source_text(
                    success_test,
                    "Eglur.toml",
                    lambda text: text.replace(
                        "packages.example.com/acme/math",
                        "packages.example.com/other/math",
                    ),
                ),
                "does not select its locked registry coordinate",
            ),
            (
                self.replace_source_text(
                    success_test,
                    "Eglur.lock",
                    lambda text: text.replace(
                        'math = { target = "$root/math" }',
                        'math = { target = "$root" }',
                    ),
                ),
                "root must select the registry node",
            ),
            (
                self.replace_source_text(
                    success_test,
                    "Eglur.lock",
                    lambda text: text.replace(
                        'math = { target = "$root/math" }',
                        'math = { target = "$root/math", path = "cache" }',
                    ),
                ),
                "must use the exact registry edge schema",
            ),
            (
                self.replace_compile_data(invalid_test, "dependency", "other"),
                "target-package diagnostic dependency",
            ),
        )
        for bundle, message in mutations:
            with self.subTest(message=message):
                problems = policy.validate_profile(profile, bundle)
                self.assertTrue(
                    any(message in problem.message for problem in problems),
                    [problem.message for problem in problems],
                )

    def test_fixture_requires_root_manifest_without_leaking_key_error(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        success_test = "MOD-qualified-registry-dependency-resolves-from-canonical-tree"
        bundle = self.remove_source_file(success_test, "Eglur.toml")

        problems = policy.validate_profile(profile, bundle)

        self.assertTrue(
            any("must contain Eglur.toml" in problem.message for problem in problems)
        )

    def test_alias_selector_expands_before_fixture_binding(self) -> None:
        mutable = _load_profile_mapping()
        cold_case = self.case_by_scenario(mutable, "cold_then_warm")
        fixture = cold_case["fixture"]
        self.assertIsInstance(fixture, dict)
        fixture["test_id"] = "MOD-registry-alias-expands-once-to-canonical-package-id"
        profile = codec.decode_profile(mutable)

        self.assertEqual(policy.validate_profile(profile, self.bundle), [])

    def test_coordinate_variants_bind_canonical_provider_tree(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        version_test = "MOD-registry-release-version-must-match-target-package-metadata"
        integrity_test = "MOD-registry-canonical-tree-integrity-mismatch-is-rejected"
        invalid_test = (
            "MOD-registry-valid-tree-without-manifest-retains-package-diagnostic"
        )
        package_test = (
            "MOD-distinct-registry-package-id-can-resolve-the-same-canonical-tree"
        )
        mutations = (
            (
                self.replace_provider_file_entry(
                    version_test,
                    b"assets/blob.bin",
                    lambda entry: replace(entry, content=b"changed"),
                ),
                "version fixture must acquire the priming package tree",
            ),
            (
                self.replace_provider_file_entry(
                    integrity_test,
                    b"src/lib.eglur",
                    lambda entry: replace(entry, path=b"src/other.eglur"),
                ),
                "integrity fixture must acquire the priming package tree",
            ),
            (
                self.replace_provider_file_entry(
                    invalid_test,
                    b"src/lib.eglur",
                    lambda entry: replace(entry, content=b"module changed\n"),
                ),
                "invalid-package fixture provider tree must match its lock",
            ),
            (
                self.replace_provider_file_entry(
                    package_test,
                    b"assets/blob.bin",
                    lambda entry: suite_model.RegistryLinkEntry(
                        kind="symlink",
                        path=entry.path,
                        target=b"target",
                    ),
                ),
                "must emit only files or directories",
            ),
        )
        for bundle, message in mutations:
            with self.subTest(message=message):
                problems = policy.validate_profile(profile, bundle)
                self.assertTrue(
                    any(message in problem.message for problem in problems),
                    [problem.message for problem in problems],
                )

    def test_integrity_mismatch_precedes_target_manifest_parsing(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        integrity_test = "MOD-registry-canonical-tree-integrity-mismatch-is-rejected"
        bundle = self.replace_provider_file_entry(
            integrity_test,
            b"Eglur.toml",
            lambda entry: replace(entry, content=b"[invalid"),
        )

        problems = policy.validate_profile(profile, bundle)

        self.assertTrue(
            any(
                "integrity fixture must acquire the priming package tree"
                in problem.message
                for problem in problems
            ),
            [problem.message for problem in problems],
        )

    def test_complete_provider_tree_is_canonical_before_hashing(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        success_test = "MOD-qualified-registry-dependency-resolves-from-canonical-tree"
        mutations = (
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"src/lib.eglur",
                    lambda entry: replace(entry, path=b"../src/lib.eglur"),
                ),
                "canonical relative POSIX path",
            ),
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"src/lib.eglur",
                    lambda entry: replace(entry, path=b"\xff"),
                ),
                "valid UTF-8 path bytes",
            ),
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"src/lib.eglur",
                    lambda entry: replace(entry, path="src/é.eglur".encode()),
                ),
                "ASCII path bytes",
            ),
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"assets/blob.bin",
                    lambda entry: replace(entry, path=b"src/LIB.eglur"),
                ),
                "case-colliding registry paths",
            ),
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"assets/blob.bin",
                    lambda entry: replace(entry, path=b"src/lib.eglur"),
                ),
                "duplicate registry path",
            ),
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"assets/blob.bin",
                    lambda entry: replace(entry, path=b"src"),
                ),
                "emits file ancestor",
            ),
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"assets/blob.bin",
                    lambda entry: replace(entry, path=b"a" * 4_097),
                ),
                "path-byte limit",
            ),
            (
                self.replace_provider_file_entry(
                    success_test,
                    b"assets/blob.bin",
                    lambda entry: replace(
                        entry,
                        path=b"/".join([b"a"] * 129),
                    ),
                ),
                "path-depth limit",
            ),
        )
        for bundle, message in mutations:
            with self.subTest(message=message):
                problems = policy.validate_profile(profile, bundle)
                self.assertTrue(
                    any(message in problem.message for problem in problems),
                    [problem.message for problem in problems],
                )

    def test_invalid_package_fixture_proves_missing_manifest_cause(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        invalid_test = (
            "MOD-registry-valid-tree-without-manifest-retains-package-diagnostic"
        )
        bundle = self.replace_provider_file_entry(
            invalid_test,
            b"src/lib.eglur",
            lambda entry: replace(
                entry,
                path=b"Eglur.toml",
                content=b'[package]\nname = "math"\nversion = "1.2.3"\n',
            ),
        )
        bundle = self.replace_source_text(
            invalid_test,
            "Eglur.lock",
            lambda text: text.replace(
                "tree-sha256:114c93dd3e1a63c54a48089dab44bbcd5578dc871cf341ad4845b5c2f504df03",
                "tree-sha256:e1566811638ceaf3f12edf0acb51a1d464716e10eb737b6f7920ed62eac36745",
            ),
            bundle=bundle,
        )

        problems = policy.validate_profile(profile, bundle)

        self.assertTrue(
            any(
                "invalid-package fixture must omit registry Eglur.toml"
                in problem.message
                for problem in problems
            )
        )

    def test_terminal_provider_failure_discards_partial_entries(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        transport_test = "MOD-registry-transport-failure-is-a-closed-outcome"
        bundle = self.replace_provider_file_entry(
            transport_test,
            b"\xff",
            lambda entry: replace(
                entry,
                path=b"Eglur.toml",
                content=b"[invalid",
            ),
        )

        self.assertEqual(policy.validate_profile(profile, bundle), [])

    def test_terminal_provider_failure_still_accounts_partial_entries(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        transport_test = "MOD-registry-transport-failure-is-a-closed-outcome"
        oversized_bundle = self.replace_provider_file_entry(
            transport_test,
            b"\xff",
            lambda entry: replace(entry, path=b"a" * 4_097),
        )
        compact_byte_bundle = self.replace_provider_file_entry(
            transport_test,
            b"\xff",
            lambda entry: replace(
                entry,
                path=suite_model.RegistryRepeatedByte(byte=97, count=1),
            ),
        )

        def repeat_entry(test: suite_model.TestCase) -> suite_model.TestCase:
            acquisition = test.invocation.registry_acquisitions[0]
            emit = acquisition.provider_steps[0]
            self.assertIsInstance(emit, suite_model.RegistryEmitStep)
            if not isinstance(emit, suite_model.RegistryEmitStep):
                self.fail("transport fixture did not start with an emit step")
            repeated = suite_model.RegistryRepeatStep(count=1, entry=emit.entry)
            return replace(
                test,
                invocation=replace(
                    test.invocation,
                    registry_acquisitions=(
                        replace(
                            acquisition,
                            provider_steps=(
                                repeated,
                                *acquisition.provider_steps[1:],
                            ),
                        ),
                    ),
                ),
            )

        repeated_bundle = self.replace_test(transport_test, repeat_entry)

        oversized_problems = policy.validate_profile(profile, oversized_bundle)
        compact_byte_problems = policy.validate_profile(profile, compact_byte_bundle)
        repeated_problems = policy.validate_profile(profile, repeated_bundle)

        self.assertTrue(
            any("path-byte limit" in problem.message for problem in oversized_problems),
            [problem.message for problem in oversized_problems],
        )
        self.assertTrue(
            any(
                "must use exact bytes" in problem.message
                for problem in compact_byte_problems
            ),
            [problem.message for problem in compact_byte_problems],
        )
        self.assertTrue(
            any("must not repeat" in problem.message for problem in repeated_problems),
            [problem.message for problem in repeated_problems],
        )

    def test_version_fixture_binds_non_root_target_node(self) -> None:
        profile = codec.decode_profile(load_yaml(_PROFILE_PATH))
        version_test = "MOD-registry-release-version-must-match-target-package-metadata"
        bundle = self.replace_compile_data(version_test, "node", "$root/other")

        problems = policy.validate_profile(profile, bundle)

        self.assertTrue(
            any(
                "version fixture node must identify its registry target"
                in problem.message
                for problem in problems
            )
        )

    def test_scenario_fixture_kinds_are_validated(self) -> None:
        success_test = "MOD-qualified-registry-dependency-resolves-from-canonical-tree"
        transport_test = "MOD-registry-transport-failure-is-a-closed-outcome"

        failed_mutation = _load_profile_mapping()
        failed_case = self.case_by_scenario(
            failed_mutation,
            "failed_acquisition_not_published",
        )
        failed_fixture = failed_case["fixture"]
        self.assertIsInstance(failed_fixture, dict)
        failed_fixture["test_id"] = success_test
        failed_profile = codec.decode_profile(failed_mutation)

        invalid_mutation = _load_profile_mapping()
        invalid_case = self.case_by_scenario(
            invalid_mutation,
            "invalid_package_not_published",
        )
        invalid_fixture = invalid_case["fixture"]
        self.assertIsInstance(invalid_fixture, dict)
        invalid_fixture["test_id"] = transport_test
        invalid_profile = codec.decode_profile(invalid_mutation)

        failed_problems = policy.validate_profile(failed_profile, self.bundle)
        invalid_problems = policy.validate_profile(invalid_profile, self.bundle)

        self.assertTrue(
            any(
                "provider failure and matching" in problem.message
                for problem in failed_problems
            )
        )
        self.assertTrue(
            any(
                "requires a complete registry acquisition" in problem.message
                for problem in invalid_problems
            )
        )

    def test_unknown_fixture_reference_is_rejected(self) -> None:
        mutation = _load_profile_mapping()
        case = self.case_by_scenario(mutation, "cold_then_warm")
        fixture = case["fixture"]
        self.assertIsInstance(fixture, dict)
        fixture["test_id"] = "MOD-does-not-exist"
        profile = codec.decode_profile(mutation)

        problems = policy.validate_profile(profile, self.bundle)

        self.assertTrue(
            any(
                "unknown registry-cache fixture" in problem.message
                for problem in problems
            )
        )

    def test_repository_rejects_manifest_other_than_profile_binding(self) -> None:
        alternate_manifest = _REPO_ROOT / "tests/conformance/probes/registry_cache.yaml"
        problems = repository.validate_repository(_REPO_ROOT, alternate_manifest)

        self.assertEqual(len(problems), 1)
        self.assertIn("does not match the selected manifest", problems[0].message)


if __name__ == "__main__":
    unittest.main()

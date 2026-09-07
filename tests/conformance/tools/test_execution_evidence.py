"""Adversarial tests for content-addressed implementation evidence."""

import unittest
from copy import deepcopy
from dataclasses import replace
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent


from eglur_conformance import artifact_content
from eglur_conformance.evidence.execution import codec as execution_codec
from eglur_conformance.evidence.execution import model as execution_model
from eglur_conformance.evidence.execution import policy as execution_policy
from eglur_conformance.evidence.execution import projection as execution_projection
from eglur_conformance.evidence.execution_context import codec as context_codec
from eglur_conformance.evidence.execution_context import model as context_model
from eglur_conformance.suite import model as suite_model
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue, load_yaml

YamlObject = dict[str, object]
OPTIONAL_PROBE_SUITE_ID = "optional-emit-if-present"
OPTIONAL_PROBE_TEST_IDS = (
    "OEP-axis-names-dropped-contract-is-validated-when-emitted",
    "OEP-axis-overlay-permuted-contract-is-validated-when-emitted",
    "OEP-traverse-opt-none-contract-is-validated-when-emitted",
    "OEP-map_axis-opt-none-contract-is-validated-when-emitted",
)
OPTIONAL_PROBE_IDENTITIES = frozenset(
    suite_model.TestIdentity(OPTIONAL_PROBE_SUITE_ID, test_id)
    for test_id in OPTIONAL_PROBE_TEST_IDS
)


def valid_context_artifact() -> YamlObject:
    """Return one complete identified-device execution context."""
    return {
        "kind": "eglur_execution_context",
        "schema_version": "0.1",
        "context": {
            "compiler": {
                "name": "egc",
                "version": "0.1.0",
                "revision": "git:0123456789abcdef0123456789abcdef01234567",
            },
            "runtime": {
                "name": "eglur-runtime",
                "version": "0.1.0",
                "revision": "git:fedcba9876543210fedcba9876543210fedcba98",
            },
            "target": {
                "triple": "x86_64-unknown-linux-gnu",
            },
            "backend": {
                "applicability": "identified",
                "name": "cpu-reference",
                "version": "1.0.0",
                "revision": f"sha256:{'a' * 64}",
            },
            "device": {
                "applicability": "identified",
                "kind": "cpu",
                "vendor": "GenuineIntel",
                "model": "reference-model",
                "ordinal": 0,
            },
            "driver_runtime": {
                "applicability": "identified",
                "name": "glibc",
                "version": "2.39",
                "revision": f"sha256:{'b' * 64}",
            },
            "determinism": {
                "default_profile": "strict",
                "randomness": {
                    "mode": "fixed_seed",
                    "algorithm": "philox4x32-10",
                    "seed": "0",
                },
            },
            "parallelism": {
                "workers": 2,
                "threads": 4,
                "processes": 1,
            },
            "active_preconditions": [
                "ffi_linkage_probe",
                "strict_unavailable_path",
            ],
        },
    }


def result_shard_artifact(
    context_digest: str,
    *,
    run_id: str = "run-2026-07-30-001",
    suite_id: str = "core-effects",
    test_id: str = "EFF-pure-call",
    result_digit: str = "2",
) -> YamlObject:
    """Return one result shard with one explicit context-bound record."""
    return {
        "kind": "eglur_result_shard",
        "schema_version": "0.1",
        "records": [
            {
                "suite_id": suite_id,
                "test_id": test_id,
                "run_id": run_id,
                "context_digest": context_digest,
                "result_digest": f"sha256:{result_digit * 64}",
            }
        ],
    }


def evidence_binding_artifact(
    context_digest: str,
    shard_digests: list[str],
    *,
    run_id: str = "run-2026-07-30-001",
) -> YamlObject:
    """Return one exact evidence-shard binding."""
    return {
        "kind": "eglur_implementation_evidence_binding",
        "schema_version": "0.1",
        "run_id": run_id,
        "context_digest": context_digest,
        "result_shard_digests": sorted(shard_digests),
    }


def raw_test_artifacts(status: str, digest_digit: str) -> tuple[bytes, bytes]:
    """Return deterministic primary-evidence and log bytes for one test."""
    primary = f"{status}-evidence-{digest_digit}".encode()
    log = f"{status}-log-{digest_digit}".encode()
    return primary, log


def test_result_artifact(
    context_digest: str,
    *,
    suite_id: str,
    test_id: str,
    status: str,
    digest_digit: str,
    run_id: str = "run-2026-07-30-001",
) -> YamlObject:
    """Return one closed status-specific test-result artifact."""
    primary, log = raw_test_artifacts(status, digest_digit)
    artifact: YamlObject = {
        "kind": "eglur_test_result",
        "schema_version": "0.1",
        "suite_id": suite_id,
        "test_id": test_id,
        "run_id": run_id,
        "context_digest": context_digest,
        "status": status,
        "log_digest": artifact_content.raw_artifact_digest(log),
    }
    if status == "not_run":
        artifact["decision_digest"] = artifact_content.raw_artifact_digest(primary)
    else:
        artifact["observed_result_digest"] = artifact_content.raw_artifact_digest(
            primary
        )
    return artifact


def conformance_inventory_artifacts(
    optional_test_ids: tuple[str, ...] = OPTIONAL_PROBE_TEST_IDS,
    *,
    include_core_extra: bool = False,
) -> tuple[YamlObject, dict[str, YamlObject]]:
    """Return a manifest and its exact suite documents for inventory tests."""
    optional_file = "optional/emit_if_present.yaml"
    manifest_suites: list[object] = [
        {
            "id": OPTIONAL_PROBE_SUITE_ID,
            "file": optional_file,
            "required": False,
        }
    ]
    suite_documents: dict[str, YamlObject] = {
        optional_file: {
            "tests": [
                {
                    "id": test_id,
                    "category": "optional_emit_if_present",
                    "title": f"Optional probe {test_id}",
                    "code": "fn main() -> Unit = ()\n",
                    "expect": {"value": {"kind": "Unit", "repr": "()"}},
                }
                for test_id in optional_test_ids
            ]
        }
    }
    if include_core_extra:
        core_file = "core/effects.yaml"
        manifest_suites.append(
            {
                "id": "core-effects",
                "file": core_file,
                "required": True,
            }
        )
        suite_documents[core_file] = {
            "tests": [
                {
                    "id": "EFF-extra",
                    "category": "effects",
                    "title": "Additional effects fixture",
                    "code": "fn main() -> Unit = ()\n",
                    "expect": {"value": {"kind": "Unit", "repr": "()"}},
                }
            ]
        }
    return {
        "version": "0.1",
        "release_history_root": "a" * 40,
        "release": "v0.1.0",
        "status": "draft",
        "root": "tests/conformance",
        "suites": manifest_suites,
    }, suite_documents


def result_shard_for_results(
    results: tuple[execution_model.TestResult, ...],
) -> execution_model.ResultShard:
    """Build one shard whose records bind the supplied result artifacts."""
    return execution_codec.parse_result_shard(
        {
            "kind": "eglur_result_shard",
            "schema_version": "0.1",
            "records": [
                {
                    "suite_id": result.suite_id,
                    "test_id": result.test_id,
                    "run_id": result.run_id,
                    "context_digest": result.context_digest,
                    "result_digest": execution_codec.test_result_digest(result),
                }
                for result in results
            ],
        }
    )


def delete_path(value: YamlObject, path: tuple[str, ...]) -> None:
    """Delete one nested mapping key from a test artifact."""
    target: object = value
    for key in path[:-1]:
        if not isinstance(target, dict):
            raise TypeError(f"test path is not a mapping at {key}")
        target = target[key]
    if not isinstance(target, dict):
        raise TypeError("test path parent is not a mapping")
    del target[path[-1]]


class ImplementationEvidenceTests(unittest.TestCase):
    """Keep result evidence explicit, immutable, and context-homogeneous."""

    def parse_valid_bundle(
        self,
    ) -> tuple[
        context_model.ExecutionContext,
        execution_model.ResultShard,
        execution_model.EvidenceBinding,
    ]:
        """Parse one valid context, result shard, and exact binding."""
        context = context_codec.parse_execution_context(valid_context_artifact())
        context_digest = context_codec.execution_context_digest(context)
        shard = execution_codec.parse_result_shard(
            result_shard_artifact(context_digest)
        )
        binding = execution_codec.parse_evidence_binding(
            evidence_binding_artifact(
                context_digest,
                [execution_codec.result_shard_digest(shard)],
            )
        )
        return context, shard, binding

    def parse_optional_probe_bundle(
        self,
        statuses: tuple[str, ...] = (
            "passed",
            "failed",
            "unsupported",
            "not_run",
        ),
    ) -> tuple[
        context_model.ExecutionContext,
        tuple[execution_model.TestResult, ...],
        execution_model.ResultShard,
        tuple[bytes, ...],
        execution_model.EvidenceBinding,
    ]:
        """Parse one exact optional-probe result set and its sign-off binding."""
        context = context_codec.parse_execution_context(valid_context_artifact())
        context_digest = context_codec.execution_context_digest(context)
        results = tuple(
            execution_codec.parse_test_result(
                test_result_artifact(
                    context_digest,
                    suite_id=OPTIONAL_PROBE_SUITE_ID,
                    test_id=test_id,
                    status=status,
                    digest_digit=str(index),
                )
            )
            for index, (test_id, status) in enumerate(
                zip(OPTIONAL_PROBE_TEST_IDS, statuses, strict=True),
                start=1,
            )
        )
        raw_artifacts = tuple(
            content
            for index, status in enumerate(statuses, start=1)
            for content in raw_test_artifacts(status, str(index))
        )
        shard = result_shard_for_results(results)
        binding = execution_codec.parse_evidence_binding(
            evidence_binding_artifact(
                context_digest,
                [execution_codec.result_shard_digest(shard)],
            )
        )
        return context, results, shard, raw_artifacts, binding

    def validate_optional_probe_evidence(
        self,
        contexts: tuple[context_model.ExecutionContext, ...],
        results: tuple[execution_model.TestResult, ...],
        shards: tuple[execution_model.ResultShard, ...],
        raw_artifacts: tuple[bytes, ...],
        binding: execution_model.EvidenceBinding,
        *,
        optional_test_ids: tuple[str, ...] = OPTIONAL_PROBE_TEST_IDS,
        include_core_extra: bool = False,
    ) -> list[ValidationProblem]:
        """Validate against inventory derived from exact manifest/suite documents."""
        manifest, suite_documents = conformance_inventory_artifacts(
            optional_test_ids,
            include_core_extra=include_core_extra,
        )
        return execution_policy.validate_optional_probe_evidence(
            contexts,
            results,
            shards,
            raw_artifacts,
            binding,
            manifest=manifest,
            suite_documents=suite_documents,
        )

    def test_valid_bundle_has_no_binding_problems(self) -> None:
        context, shard, binding = self.parse_valid_bundle()

        self.assertEqual(
            execution_policy.validate_evidence_binding(
                (context,),
                (shard,),
                binding,
            ),
            [],
        )

    def test_optional_probe_states_have_distinct_closed_shapes(self) -> None:
        context = context_codec.parse_execution_context(valid_context_artifact())
        context_digest = context_codec.execution_context_digest(context)
        for index, status in enumerate(
            ("passed", "failed", "unsupported", "not_run"),
            start=1,
        ):
            with self.subTest(status=status):
                result = execution_codec.parse_test_result(
                    test_result_artifact(
                        context_digest,
                        suite_id=OPTIONAL_PROBE_SUITE_ID,
                        test_id=OPTIONAL_PROBE_TEST_IDS[index - 1],
                        status=status,
                        digest_digit=str(index),
                    )
                )
                self.assertEqual(result.status, status)
                if status == "not_run":
                    self.assertIsInstance(
                        result,
                        execution_model.NotRunTestResult,
                    )
                    self.assertFalse(hasattr(result, "observed_result_digest"))
                else:
                    self.assertIsInstance(
                        result,
                        execution_model.ObservedTestResult,
                    )
                    self.assertFalse(hasattr(result, "decision_digest"))
                self.assertRegex(
                    execution_codec.test_result_digest(result),
                    r"^sha256:[0-9a-f]{64}$",
                )

    def test_optional_probe_state_shapes_reject_missing_and_cross_variant_fields(
        self,
    ) -> None:
        context = context_codec.parse_execution_context(valid_context_artifact())
        context_digest = context_codec.execution_context_digest(context)
        executed = test_result_artifact(
            context_digest,
            suite_id=OPTIONAL_PROBE_SUITE_ID,
            test_id=OPTIONAL_PROBE_TEST_IDS[0],
            status="passed",
            digest_digit="1",
        )
        not_run = test_result_artifact(
            context_digest,
            suite_id=OPTIONAL_PROBE_SUITE_ID,
            test_id=OPTIONAL_PROBE_TEST_IDS[1],
            status="not_run",
            digest_digit="2",
        )
        mutations: list[YamlObject] = []
        for key in ("observed_result_digest", "log_digest"):
            mutation = deepcopy(executed)
            del mutation[key]
            mutations.append(mutation)
        executed_with_decision = deepcopy(executed)
        executed_with_decision["decision_digest"] = f"sha256:{'3' * 64}"
        mutations.append(executed_with_decision)
        for key in ("decision_digest", "log_digest"):
            mutation = deepcopy(not_run)
            del mutation[key]
            mutations.append(mutation)
        not_run_with_result = deepcopy(not_run)
        not_run_with_result["observed_result_digest"] = f"sha256:{'4' * 64}"
        mutations.append(not_run_with_result)

        for mutation in mutations:
            with (
                self.subTest(mutation=mutation),
                self.assertRaisesRegex(ValueError, "missing|unsupported"),
            ):
                execution_codec.parse_test_result(mutation)

        unknown = deepcopy(executed)
        unknown["status"] = "skip"
        with self.assertRaisesRegex(ValueError, "must be one of"):
            execution_codec.parse_test_result(unknown)

        malformed = deepcopy(executed)
        malformed["log_digest"] = "run-1"
        with self.assertRaisesRegex(ValueError, "64 lowercase hex"):
            execution_codec.parse_test_result(malformed)

    def test_optional_probe_evidence_binds_every_expected_result(self) -> None:
        context, results, shard, raw_artifacts, binding = (
            self.parse_optional_probe_bundle()
        )

        self.assertEqual(
            self.validate_optional_probe_evidence(
                (context,),
                results,
                (shard,),
                raw_artifacts,
                binding,
            ),
            [],
        )

    def test_omitted_optional_probe_result_fails_even_with_a_bound_run(self) -> None:
        context, results, _, raw_artifacts, _ = self.parse_optional_probe_bundle()
        partial_results = results[:-1]
        shard = result_shard_for_results(partial_results)
        binding = execution_codec.parse_evidence_binding(
            evidence_binding_artifact(
                context_codec.execution_context_digest(context),
                [execution_codec.result_shard_digest(shard)],
                run_id="run-1",
            )
        )
        partial_results = tuple(
            replace(result, run_id="run-1") for result in partial_results
        )
        shard = result_shard_for_results(partial_results)
        binding = replace(
            binding,
            result_shard_digests=(execution_codec.result_shard_digest(shard),),
        )

        messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                partial_results,
                (shard,),
                raw_artifacts[:-2],
                binding,
            )
        ]
        self.assertTrue(
            any("missing optional probe result" in item for item in messages)
        )

    def test_result_artifacts_must_match_shard_references_exactly(self) -> None:
        context, results, shard, raw_artifacts, binding = (
            self.parse_optional_probe_bundle()
        )

        missing_messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                results[:-1],
                (shard,),
                raw_artifacts,
                binding,
            )
        ]
        self.assertTrue(
            any(
                "referenced test result artifact is missing" in item
                for item in missing_messages
            )
        )

        extra_result = replace(
            results[0],
            suite_id="core-effects",
            test_id="EFF-extra",
        )
        extra_messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                (*results, extra_result),
                (shard,),
                raw_artifacts,
                binding,
                include_core_extra=True,
            )
        ]
        self.assertTrue(
            any(
                "test result artifact is not shard-bound" in item
                for item in extra_messages
            )
        )

    def test_raw_result_log_and_decision_bytes_are_bound_exactly(self) -> None:
        context, results, shard, raw_artifacts, binding = (
            self.parse_optional_probe_bundle()
        )

        missing_messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                results,
                (shard,),
                raw_artifacts[:-1],
                binding,
            )
        ]
        self.assertTrue(
            any("raw artifact is missing" in item for item in missing_messages)
        )

        extra_messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                results,
                (shard,),
                (*raw_artifacts, b"unbound-log"),
                binding,
            )
        ]
        self.assertTrue(
            any("raw artifact is not bound" in item for item in extra_messages)
        )

        tampered = (*raw_artifacts[:-1], raw_artifacts[-1] + b"-tampered")
        tampered_messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                results,
                (shard,),
                tampered,
                binding,
            )
        ]
        self.assertTrue(
            any("raw artifact is missing" in item for item in tampered_messages)
        )
        self.assertTrue(
            any("raw artifact is not bound" in item for item in tampered_messages)
        )

        duplicate_messages = [
            problem.message
            for problem in execution_policy.validate_raw_artifact_binding(
                results,
                (*raw_artifacts, raw_artifacts[0]),
            )
        ]
        self.assertTrue(
            any("duplicate raw artifact content" in item for item in duplicate_messages)
        )

    def test_result_shard_and_result_artifact_identities_must_agree(self) -> None:
        context, results, shard, raw_artifacts, binding = (
            self.parse_optional_probe_bundle()
        )
        mismatched_record = replace(shard.records[0], test_id="OEP-wrong")
        mismatched_shard = replace(
            shard,
            records=(mismatched_record, *shard.records[1:]),
        )
        mismatched_binding = replace(
            binding,
            result_shard_digests=(
                execution_codec.result_shard_digest(mismatched_shard),
            ),
        )

        messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                results,
                (mismatched_shard,),
                raw_artifacts,
                mismatched_binding,
            )
        ]
        self.assertTrue(any("result shard identity" in item for item in messages))

    def test_optional_probe_status_changes_test_result_address(self) -> None:
        _, results, _, _, _ = self.parse_optional_probe_bundle()
        passed = results[0]
        if not isinstance(passed, execution_model.ObservedTestResult):
            self.fail("first optional probe result must be observed")
        failed = replace(passed, status="failed")

        self.assertNotEqual(
            execution_codec.test_result_digest(passed),
            execution_codec.test_result_digest(failed),
        )

    def test_optional_probe_inventory_comes_from_manifest_suite_documents(self) -> None:
        context, results, shard, raw_artifacts, binding = (
            self.parse_optional_probe_bundle()
        )
        extra_result = replace(
            results[0],
            suite_id="core-effects",
            test_id="EFF-unknown",
        )
        messages = [
            problem.message
            for problem in self.validate_optional_probe_evidence(
                (context,),
                (*results, extra_result),
                (shard,),
                raw_artifacts,
                binding,
            )
        ]
        self.assertTrue(
            any("unknown test result identity" in item for item in messages)
        )

        manifest, suite_documents = conformance_inventory_artifacts()
        inventory = execution_projection.parse_conformance_test_inventory(
            manifest,
            suite_documents,
        )
        self.assertEqual(inventory.known_test_identities, OPTIONAL_PROBE_IDENTITIES)
        self.assertEqual(
            inventory.optional_probe_test_identities,
            OPTIONAL_PROBE_IDENTITIES,
        )

    def test_optional_probe_inventory_requires_the_exact_manifest_suite_set(
        self,
    ) -> None:
        manifest, suite_documents = conformance_inventory_artifacts()
        suite_file = next(iter(suite_documents))

        with self.assertRaisesRegex(ValueError, "do not match manifest exactly"):
            execution_projection.parse_conformance_test_inventory(manifest, {})
        with self.assertRaisesRegex(ValueError, "do not match manifest exactly"):
            execution_projection.parse_conformance_test_inventory(
                manifest,
                {**suite_documents, "optional/unbound.yaml": {"tests": []}},
            )

        required_manifest = deepcopy(manifest)
        raw_suites = required_manifest["suites"]
        self.assertIsInstance(raw_suites, list)
        required_suite = raw_suites[0]
        self.assertIsInstance(required_suite, dict)
        required_suite["required"] = True
        with self.assertRaisesRegex(ValueError, "inside a required suite"):
            execution_projection.parse_conformance_test_inventory(
                required_manifest,
                suite_documents,
            )

        non_probe_documents = deepcopy(suite_documents)
        raw_tests = non_probe_documents[suite_file]["tests"]
        self.assertIsInstance(raw_tests, list)
        first_test = raw_tests[0]
        self.assertIsInstance(first_test, dict)
        first_test["category"] = "effects"
        inventory = execution_projection.parse_conformance_test_inventory(
            manifest,
            non_probe_documents,
        )
        self.assertNotEqual(
            inventory.known_test_identities,
            inventory.optional_probe_test_identities,
        )

    def test_repository_manifest_owns_optional_probe_inventory(self) -> None:
        conformance_root = TOOLS_DIR.parent
        manifest = load_yaml(conformance_root / "manifest.yaml")
        self.assertIsInstance(manifest, dict)
        raw_suites = manifest["suites"]
        self.assertIsInstance(raw_suites, list)
        suite_documents: dict[str, YamlValue] = {}
        for raw_suite in raw_suites:
            self.assertIsInstance(raw_suite, dict)
            suite_file = raw_suite["file"]
            self.assertIsInstance(suite_file, str)
            suite_documents[suite_file] = load_yaml(conformance_root / suite_file)

        inventory = execution_projection.parse_conformance_test_inventory(
            manifest,
            suite_documents,
        )
        self.assertEqual(
            inventory.optional_probe_test_identities,
            OPTIONAL_PROBE_IDENTITIES,
        )

    def test_context_digest_is_independent_of_yaml_key_order(self) -> None:
        artifact = valid_context_artifact()
        context_body = artifact["context"]
        self.assertIsInstance(context_body, dict)
        reordered = {
            "kind": artifact["kind"],
            "schema_version": artifact["schema_version"],
            "context": dict(reversed(list(context_body.items()))),
        }

        original_context = context_codec.parse_execution_context(artifact)
        reordered_context = context_codec.parse_execution_context(reordered)

        self.assertEqual(
            context_codec.execution_context_digest(original_context),
            context_codec.execution_context_digest(reordered_context),
        )

    def test_canonical_json_bytes_and_digest_match_the_golden_vector(self) -> None:
        value: artifact_content.JsonValue = {
            "z": "é/\n\u0001",
            "a": [1, True, None],
        }
        expected = b'{"a":[1,true,null],"z":"\xc3\xa9/\\n\\u0001"}'

        self.assertEqual(artifact_content.canonical_json_bytes(value), expected)
        self.assertEqual(
            artifact_content.canonical_digest(value),
            "sha256:4600ee560ab748b1e7c88bbbac101a9c0bb37fcb46de62922a655954d3bcbef2",
        )

    def test_lone_surrogates_are_rejected_at_string_ingress(self) -> None:
        context_artifact = valid_context_artifact()
        context_body = context_artifact["context"]
        self.assertIsInstance(context_body, dict)
        compiler = context_body["compiler"]
        self.assertIsInstance(compiler, dict)
        compiler["name"] = "\ud800"
        with self.assertRaisesRegex(ValueError, "lone surrogate"):
            context_codec.parse_execution_context(context_artifact)

        context, results, shard, _, binding = self.parse_optional_probe_bundle()
        invalid_context = replace(
            context,
            compiler=replace(context.compiler, name="\ud800"),
        )
        context_messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (invalid_context,),
                (shard,),
                binding,
            )
        ]
        self.assertTrue(
            any(
                "invalid execution context artifact" in item
                for item in context_messages
            )
        )

        invalid_result = replace(results[0], test_id="\ud800")
        result_messages = [
            problem.message
            for problem in execution_policy.validate_test_result_binding(
                (invalid_result,),
                (),
            )
        ]
        self.assertTrue(
            any("invalid test result artifact" in item for item in result_messages)
        )

    def test_context_rejects_every_required_field_when_missing(self) -> None:
        required_paths = (
            ("kind",),
            ("schema_version",),
            ("context",),
            ("context", "compiler"),
            ("context", "runtime"),
            ("context", "target"),
            ("context", "backend"),
            ("context", "device"),
            ("context", "driver_runtime"),
            ("context", "determinism"),
            ("context", "parallelism"),
            ("context", "active_preconditions"),
            ("context", "compiler", "revision"),
            ("context", "runtime", "revision"),
            ("context", "target", "triple"),
            ("context", "backend", "version"),
            ("context", "backend", "revision"),
            ("context", "device", "model"),
            ("context", "driver_runtime", "version"),
            ("context", "driver_runtime", "revision"),
            ("context", "determinism", "default_profile"),
            ("context", "determinism", "randomness"),
            ("context", "parallelism", "workers"),
            ("context", "parallelism", "threads"),
            ("context", "parallelism", "processes"),
        )
        for path in required_paths:
            with self.subTest(path=path):
                artifact = valid_context_artifact()
                delete_path(artifact, path)
                with self.assertRaisesRegex(ValueError, "missing"):
                    context_codec.parse_execution_context(artifact)

    def test_not_applicable_components_use_closed_discriminated_shapes(self) -> None:
        artifact = valid_context_artifact()
        body = artifact["context"]
        self.assertIsInstance(body, dict)
        body["backend"] = {"applicability": "not_applicable"}
        body["device"] = {"applicability": "not_applicable"}
        body["driver_runtime"] = {"applicability": "not_applicable"}

        parsed = context_codec.parse_execution_context(artifact)
        self.assertIsInstance(
            parsed.backend,
            context_model.NotApplicableComponent,
        )
        self.assertIsInstance(
            parsed.device,
            context_model.NotApplicableDevice,
        )
        self.assertIsInstance(
            parsed.driver_runtime,
            context_model.NotApplicableComponent,
        )

        invalid = deepcopy(artifact)
        invalid_body = invalid["context"]
        self.assertIsInstance(invalid_body, dict)
        invalid_body["backend"] = {
            "applicability": "not_applicable",
            "name": "none",
        }
        with self.assertRaisesRegex(ValueError, "unsupported"):
            context_codec.parse_execution_context(invalid)

    def test_randomness_variants_are_closed(self) -> None:
        for randomness in (
            {"mode": "unused"},
            {"mode": "fixed_seed", "algorithm": "pcg64", "seed": "17"},
            {"mode": "system_entropy", "source": "getrandom-v2"},
        ):
            with self.subTest(randomness=randomness):
                artifact = valid_context_artifact()
                body = artifact["context"]
                self.assertIsInstance(body, dict)
                determinism = body["determinism"]
                self.assertIsInstance(determinism, dict)
                determinism["randomness"] = randomness
                context_codec.parse_execution_context(artifact)

        invalid = valid_context_artifact()
        invalid_body = invalid["context"]
        self.assertIsInstance(invalid_body, dict)
        invalid_determinism = invalid_body["determinism"]
        self.assertIsInstance(invalid_determinism, dict)
        invalid_determinism["randomness"] = {
            "mode": "unused",
            "seed": "0",
        }
        with self.assertRaisesRegex(ValueError, "unsupported"):
            context_codec.parse_execution_context(invalid)

    def test_context_digest_changes_for_every_execution_sensitive_field(self) -> None:
        original = valid_context_artifact()
        original_context = context_codec.parse_execution_context(original)
        original_digest = context_codec.execution_context_digest(original_context)
        mutations = (
            (
                ("context", "compiler", "revision"),
                f"sha256:{'a' * 64}",
            ),
            (
                ("context", "runtime", "revision"),
                f"sha256:{'b' * 64}",
            ),
            (("context", "target", "triple"), "aarch64-unknown-linux-gnu"),
            (("context", "backend", "name"), "gpu-reference"),
            (
                ("context", "backend", "revision"),
                f"sha256:{'c' * 64}",
            ),
            (("context", "device", "model"), "different-device"),
            (("context", "driver_runtime", "version"), "2.40"),
            (
                ("context", "driver_runtime", "revision"),
                f"sha256:{'d' * 64}",
            ),
            (("context", "determinism", "default_profile"), "default"),
            (("context", "determinism", "randomness", "seed"), "1"),
            (("context", "parallelism", "workers"), 3),
            (("context", "parallelism", "threads"), 5),
            (("context", "parallelism", "processes"), 2),
            (
                ("context", "active_preconditions"),
                [
                    "console_write_failure_probe",
                    "ffi_linkage_probe",
                    "par_traverse_quiescence_probe",
                    "scoped_close_failure_probe",
                    "strict_unavailable_path",
                ],
            ),
        )
        for path, replacement in mutations:
            with self.subTest(path=path):
                artifact = deepcopy(original)
                target: object = artifact
                for key in path[:-1]:
                    self.assertIsInstance(target, dict)
                    target = target[key]
                self.assertIsInstance(target, dict)
                target[path[-1]] = replacement
                mutated = context_codec.parse_execution_context(artifact)
                self.assertNotEqual(
                    context_codec.execution_context_digest(mutated),
                    original_digest,
                )

    def test_mutated_context_cannot_reuse_the_original_binding(self) -> None:
        original_context, shard, binding = self.parse_valid_bundle()
        original_digest = context_codec.execution_context_digest(original_context)
        self.assertEqual(binding.context_digest, original_digest)
        mutations = (
            (("context", "backend", "version"), "2.0.0"),
            (("context", "device", "ordinal"), 1),
            (("context", "determinism", "default_profile"), "default"),
            (("context", "parallelism", "threads"), 8),
        )
        for path, replacement in mutations:
            with self.subTest(path=path):
                artifact = valid_context_artifact()
                target: object = artifact
                for key in path[:-1]:
                    self.assertIsInstance(target, dict)
                    target = target[key]
                self.assertIsInstance(target, dict)
                target[path[-1]] = replacement
                mutated = context_codec.parse_execution_context(artifact)
                messages = [
                    problem.message
                    for problem in execution_policy.validate_evidence_binding(
                        (mutated,),
                        (shard,),
                        binding,
                    )
                ]
                self.assertTrue(
                    any(
                        "no matching execution context" in message
                        for message in messages
                    )
                )
                self.assertTrue(
                    any("unbound execution context" in message for message in messages)
                )

    def test_readdressed_context_rejects_stale_result_records(self) -> None:
        _, shard, _ = self.parse_valid_bundle()
        artifact = valid_context_artifact()
        body = artifact["context"]
        self.assertIsInstance(body, dict)
        parallelism = body["parallelism"]
        self.assertIsInstance(parallelism, dict)
        parallelism["workers"] = 7
        context = context_codec.parse_execution_context(artifact)
        context_digest = context_codec.execution_context_digest(context)
        binding = execution_codec.parse_evidence_binding(
            evidence_binding_artifact(
                context_digest,
                [execution_codec.result_shard_digest(shard)],
            )
        )

        messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (context,),
                (shard,),
                binding,
            )
        ]
        self.assertTrue(any("has context" in message for message in messages))

    def test_mixed_context_and_mixed_run_records_are_rejected(self) -> None:
        context, _, _ = self.parse_valid_bundle()
        context_digest = context_codec.execution_context_digest(context)
        other_context_artifact = valid_context_artifact()
        other_body = other_context_artifact["context"]
        self.assertIsInstance(other_body, dict)
        other_device = other_body["device"]
        self.assertIsInstance(other_device, dict)
        other_device["model"] = "other-model"
        other_context = context_codec.parse_execution_context(other_context_artifact)
        other_digest = context_codec.execution_context_digest(other_context)

        shard_artifact = result_shard_artifact(context_digest)
        records = shard_artifact["records"]
        self.assertIsInstance(records, list)
        records.append(
            {
                "suite_id": "core-parser",
                "test_id": "PAR-valid",
                "run_id": "other-run",
                "context_digest": other_digest,
                "result_digest": f"sha256:{'3' * 64}",
            }
        )
        shard = execution_codec.parse_result_shard(shard_artifact)
        binding = execution_codec.parse_evidence_binding(
            evidence_binding_artifact(
                context_digest,
                [execution_codec.result_shard_digest(shard)],
            )
        )

        messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (context,),
                (shard,),
                binding,
            )
        ]
        self.assertTrue(any("has run_id" in message for message in messages))
        self.assertTrue(any("has context" in message for message in messages))

    def test_binding_must_name_the_exact_provided_shard_set(self) -> None:
        context, shard, binding = self.parse_valid_bundle()
        context_digest = context_codec.execution_context_digest(context)
        extra_shard = execution_codec.parse_result_shard(
            result_shard_artifact(
                context_digest,
                suite_id="core-parser",
                test_id="PAR-valid",
                result_digit="3",
            )
        )

        missing_messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (context,),
                (),
                binding,
            )
        ]
        self.assertTrue(
            any(
                "bound result shard artifact is missing" in item
                for item in missing_messages
            )
        )

        extra_messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (context,),
                (shard, extra_shard),
                binding,
            )
        ]
        self.assertTrue(
            any("result shard artifact is not bound" in item for item in extra_messages)
        )

    def test_duplicate_test_records_across_shards_are_rejected(self) -> None:
        context, first_shard, _ = self.parse_valid_bundle()
        context_digest = context_codec.execution_context_digest(context)
        second_shard = execution_codec.parse_result_shard(
            result_shard_artifact(context_digest, result_digit="4")
        )
        shard_digests = [
            execution_codec.result_shard_digest(first_shard),
            execution_codec.result_shard_digest(second_shard),
        ]
        binding = execution_codec.parse_evidence_binding(
            evidence_binding_artifact(context_digest, shard_digests)
        )

        messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (context,),
                (first_shard, second_shard),
                binding,
            )
        ]
        self.assertTrue(
            any("duplicate test-level result record" in item for item in messages)
        )

    def test_result_records_cannot_omit_or_fake_context_addresses(self) -> None:
        context = context_codec.parse_execution_context(valid_context_artifact())
        context_digest = context_codec.execution_context_digest(context)

        missing = result_shard_artifact(context_digest)
        missing_records = missing["records"]
        self.assertIsInstance(missing_records, list)
        self.assertIsInstance(missing_records[0], dict)
        del missing_records[0]["context_digest"]
        with self.assertRaisesRegex(ValueError, "missing"):
            execution_codec.parse_result_shard(missing)

        malformed = result_shard_artifact(context_digest)
        malformed_records = malformed["records"]
        self.assertIsInstance(malformed_records, list)
        self.assertIsInstance(malformed_records[0], dict)
        malformed_records[0]["context_digest"] = "sha256:ABC"
        with self.assertRaisesRegex(ValueError, "64 lowercase hex"):
            execution_codec.parse_result_shard(malformed)

    def test_binding_rejects_unsorted_duplicate_or_empty_shard_addresses(self) -> None:
        context = context_codec.parse_execution_context(valid_context_artifact())
        context_digest = context_codec.execution_context_digest(context)
        first = f"sha256:{'1' * 64}"
        second = f"sha256:{'2' * 64}"

        for digests, message in (
            ([], "must not be empty"),
            ([second, first], "must be sorted"),
            ([first, first], "must not contain duplicates"),
        ):
            with self.subTest(digests=digests):
                artifact = evidence_binding_artifact(context_digest, digests)
                artifact["result_shard_digests"] = digests
                with self.assertRaisesRegex(ValueError, message):
                    execution_codec.parse_evidence_binding(artifact)

    def test_preconditions_are_closed_sorted_and_duplicate_free(self) -> None:
        invalid_values = (
            (
                ["strict_unavailable_path", "ffi_linkage_probe"],
                "must be sorted",
            ),
            (
                ["ffi_linkage_probe", "ffi_linkage_probe"],
                "must not contain duplicates",
            ),
            (["future_probe"], "unknown v0.1"),
        )
        for preconditions, message in invalid_values:
            with self.subTest(preconditions=preconditions):
                artifact = valid_context_artifact()
                body = artifact["context"]
                self.assertIsInstance(body, dict)
                body["active_preconditions"] = preconditions
                with self.assertRaisesRegex(ValueError, message):
                    context_codec.parse_execution_context(artifact)

    def test_context_rejects_placeholders_zero_counts_and_unknown_fields(self) -> None:
        for revision in (
            "TBD",
            "git:unknown",
            "main",
            f"git:{'a' * 39}",
            f"sha256:{'A' * 64}",
            f"git:{'0' * 40}",
            f"git:{'0' * 64}",
        ):
            with self.subTest(revision=revision):
                placeholder = valid_context_artifact()
                placeholder_body = placeholder["context"]
                self.assertIsInstance(placeholder_body, dict)
                compiler = placeholder_body["compiler"]
                self.assertIsInstance(compiler, dict)
                compiler["revision"] = revision
                with self.assertRaisesRegex(ValueError, "must"):
                    context_codec.parse_execution_context(placeholder)

        for component_name in ("backend", "driver_runtime"):
            for revision in ("nightly", f"git:{'0' * 40}"):
                with self.subTest(component=component_name, revision=revision):
                    placeholder = valid_context_artifact()
                    placeholder_body = placeholder["context"]
                    self.assertIsInstance(placeholder_body, dict)
                    component = placeholder_body[component_name]
                    self.assertIsInstance(component, dict)
                    component["revision"] = revision
                    with self.assertRaisesRegex(ValueError, "must"):
                        context_codec.parse_execution_context(placeholder)

        zero_workers = valid_context_artifact()
        zero_body = zero_workers["context"]
        self.assertIsInstance(zero_body, dict)
        parallelism = zero_body["parallelism"]
        self.assertIsInstance(parallelism, dict)
        parallelism["workers"] = 0
        with self.assertRaisesRegex(ValueError, "must be positive"):
            context_codec.parse_execution_context(zero_workers)

        unknown = valid_context_artifact()
        unknown["extra"] = True
        with self.assertRaisesRegex(ValueError, "unsupported"):
            context_codec.parse_execution_context(unknown)

        redundant_target = valid_context_artifact()
        redundant_body = redundant_target["context"]
        self.assertIsInstance(redundant_body, dict)
        target = redundant_body["target"]
        self.assertIsInstance(target, dict)
        target["architecture"] = "x86_64"
        with self.assertRaisesRegex(ValueError, "unsupported"):
            context_codec.parse_execution_context(redundant_target)

    def test_validator_rechecks_directly_constructed_binding_invariants(self) -> None:
        context = context_codec.parse_execution_context(valid_context_artifact())
        context_digest = context_codec.execution_context_digest(context)
        empty_binding = execution_model.EvidenceBinding(
            run_id="run-2026-07-30-001",
            context_digest=context_digest,
            result_shard_digests=(),
        )

        empty_binding_messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (context,),
                (),
                empty_binding,
            )
        ]
        self.assertTrue(
            any(
                "result_shard_digests must not be empty" in item
                for item in empty_binding_messages
            )
        )

        empty_shard = execution_model.ResultShard(records=())
        empty_shard_digest = execution_codec.result_shard_digest(empty_shard)
        binding = execution_model.EvidenceBinding(
            run_id="run-2026-07-30-001",
            context_digest=context_digest,
            result_shard_digests=(empty_shard_digest,),
        )
        empty_shard_messages = [
            problem.message
            for problem in execution_policy.validate_evidence_binding(
                (context,),
                (empty_shard,),
                binding,
            )
        ]
        self.assertTrue(
            any("records must not be empty" in item for item in empty_shard_messages)
        )

    def test_validator_reparses_all_directly_constructed_models(self) -> None:
        context, shard, binding = self.parse_valid_bundle()
        invalid_contexts = (
            replace(
                context,
                compiler=replace(context.compiler, revision="main"),
            ),
            replace(context, target=context_model.Target(triple="")),
            replace(
                context,
                parallelism=context_model.Parallelism(
                    workers=0,
                    threads=4,
                    processes=1,
                ),
            ),
            replace(
                context,
                backend=context_model.RevisionedComponent(
                    name="",
                    version="nightly",
                    revision=f"sha256:{'e' * 64}",
                ),
            ),
            replace(
                context,
                device=context_model.IdentifiedDevice(
                    kind="gpu",
                    vendor="",
                    model="hidden-model",
                    ordinal=0,
                ),
            ),
            replace(
                context,
                determinism=replace(
                    context.determinism,
                    randomness=context_model.FixedSeedRandomness(
                        algorithm="",
                        seed="1",
                    ),
                ),
            ),
        )
        for invalid_context in invalid_contexts:
            with self.subTest(invalid_context=invalid_context):
                messages = [
                    problem.message
                    for problem in execution_policy.validate_evidence_binding(
                        (invalid_context,),
                        (shard,),
                        binding,
                    )
                ]
                self.assertTrue(
                    any(
                        "invalid execution context artifact" in item
                        for item in messages
                    )
                )

        record = shard.records[0]
        invalid_records = (
            replace(record, suite_id=""),
            replace(record, test_id=""),
            replace(record, run_id=""),
            replace(record, context_digest="not-a-digest"),
            replace(record, result_digest="not-a-digest"),
        )
        for invalid_record in invalid_records:
            with self.subTest(invalid_record=invalid_record):
                invalid_shard = execution_model.ResultShard(records=(invalid_record,))
                invalid_shard_digest = execution_codec.result_shard_digest(
                    invalid_shard
                )
                invalid_binding = replace(
                    binding,
                    result_shard_digests=(invalid_shard_digest,),
                )
                messages = [
                    problem.message
                    for problem in execution_policy.validate_evidence_binding(
                        (context,),
                        (invalid_shard,),
                        invalid_binding,
                    )
                ]
                self.assertTrue(
                    any("invalid result shard artifact" in item for item in messages)
                )

    def test_test_result_variants_cannot_store_hidden_cross_variant_state(self) -> None:
        _, results, _, _, _ = self.parse_optional_probe_bundle()
        for result, forbidden_field, digest_digit in (
            (results[-1], "observed_result_digest", "f"),
            (results[0], "decision_digest", "e"),
        ):
            with (
                self.subTest(forbidden_field=forbidden_field),
                self.assertRaises(TypeError),
            ):
                replace(
                    result,
                    **{forbidden_field: f"sha256:{digest_digit * 64}"},
                )


if __name__ == "__main__":
    unittest.main()

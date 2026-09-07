"""Mutation tests for closed conformance and release artifact schemas."""

import subprocess
import sys
import tempfile
import unittest
from collections.abc import Callable
from copy import deepcopy
from dataclasses import replace
from pathlib import Path

import yaml

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]

from eglur_conformance import release_identity
from eglur_conformance.spec_release import (
    gate,
    open_items,
    release_state,
    repository,
)
from eglur_conformance.spec_release.signoff import codec as signoff_codec
from eglur_conformance.spec_release.signoff import policy as signoff_policy
from eglur_conformance.suite import artifact as suite_artifact
from eglur_conformance.suite import schema as conformance_schema
from eglur_conformance.traceability import codec as traceability_codec
from eglur_conformance.traceability import model as traceability_model
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue, load_yaml


class ArtifactSchemaMutationTests(unittest.TestCase):
    """Require versions, statuses, types, and fields to fail closed."""

    def release_fixture(self) -> repository.SpecPackageSnapshot:
        data = repository.load_snapshot(REPO_ROOT)
        return replace(
            data,
            inventory=replace(
                data.inventory,
                release_history_root="b" * 40,
                release="v0.1.0",
                status="draft",
            ),
            manifest=replace(
                data.manifest,
                release_history_root="b" * 40,
                release="v0.1.0",
                status="draft",
            ),
            traceability=replace(
                data.traceability,
                release_history_root="b" * 40,
                release=release_identity.parse_release_label("v0.1.0", "fixture"),
                status="draft",
            ),
            ledger=replace(
                data.ledger,
                release_history_root="b" * 40,
                release="v0.1.0",
                status="draft",
            ),
        )

    def load_ledger(self, path: Path) -> open_items.Ledger:
        """Decode one OPEN-item ledger fixture."""
        return open_items.decode_ledger(load_yaml(path), str(path))

    def test_signoff_timestamp_requires_explicit_utc(self) -> None:
        for value in ("2026-07-30T00:00:00Z", "2026-07-30T00:00:00+00:00"):
            with self.subTest(value=value):
                self.assertEqual(
                    signoff_policy.validate_timestamp(value, "timestamp"), []
                )

        invalid = {
            "not-a-timestamp": "timestamp must be an ISO-8601 timestamp",
            "2026-07-30T00:00:00": "timestamp must include a UTC offset",
            "2026-07-30T00:00:00+01:00": "timestamp must be UTC",
        }
        for value, message in invalid.items():
            with self.subTest(value=value):
                self.assertEqual(
                    [
                        problem.message
                        for problem in signoff_policy.validate_timestamp(
                            value,
                            "timestamp",
                        )
                    ],
                    [message],
                )

    def validate_signoff(
        self,
        snapshot: repository.SpecPackageSnapshot,
        path: Path,
        commit: str,
    ) -> list[ValidationProblem]:
        """Decode and validate one sign-off fixture."""
        signoff = signoff_codec.decode_spec_package_signoff(load_yaml(path))
        return gate.validate_signoff(snapshot, signoff, commit)

    def write_yaml(self, directory: str, value: object) -> Path:
        """Serialize one synthetic artifact."""
        path = Path(directory) / "artifact.yaml"
        rendered = yaml.safe_dump(value, sort_keys=False)
        if not isinstance(rendered, str):
            raise TypeError("synthetic YAML serialization must return text")
        path.write_text(rendered, encoding="utf-8")
        return path

    def test_manifest_rejects_wrong_version_status_and_extra_field(self) -> None:
        base = {
            "version": "0.1",
            "release_history_root": "a" * 40,
            "release": "v0.1.0",
            "status": "draft",
            "root": "tests/conformance",
            "suites": [],
        }
        mutations = (
            ({**base, "version": "9.9"}, "manifest.version"),
            (
                {**base, "release_history_root": "abc"},
                "manifest.release_history_root",
            ),
            ({**base, "release": "v0.1.00"}, "manifest.release"),
            ({**base, "status": "unknown"}, "manifest.status"),
            ({**base, "root": "/tmp/outside"}, "manifest.root"),
            ({**base, "extra": True}, "unsupported"),
        )

        with tempfile.TemporaryDirectory() as directory:
            for value, message in mutations:
                with self.subTest(value=value):
                    path = self.write_yaml(directory, value)
                    with self.assertRaisesRegex(ValueError, message):
                        conformance_schema.parse_manifest(path)

    def test_suite_rejects_unknown_root_test_and_value_fields(self) -> None:
        fixture = {
            "id": "SYN-closed-schema",
            "category": "synthetic",
            "title": "closed schema",
            "code": 'fn main!() -> Res[Str, Str] { yield Ok("ok") }\n',
            "expect": {"value": {"kind": "Ok", "repr": '"ok"'}},
        }
        mutations = (
            {"tests": [fixture], "extra": True},
            {"tests": [{**fixture, "extra": True}]},
            {
                "tests": [
                    {
                        **fixture,
                        "expect": {
                            "value": {
                                "kind": "Ok",
                                "repr": '"ok"',
                                "extra": True,
                            }
                        },
                    }
                ]
            },
        )

        with tempfile.TemporaryDirectory() as directory:
            for value in mutations:
                with self.subTest(value=value):
                    path = self.write_yaml(directory, value)
                    _, problems = conformance_schema.validate_suite_file(
                        path,
                        "synthetic.yaml",
                    )
                    self.assertTrue(
                        any("not a supported key" in item.message for item in problems)
                    )

    def test_open_item_ledger_rejects_version_status_and_extra_field(self) -> None:
        ledger = load_yaml(REPO_ROOT / open_items.DEFAULT_LEDGER_PATH)
        self.assertIsInstance(ledger, dict)

        with tempfile.TemporaryDirectory() as directory:
            wrong_version = dict(ledger)
            wrong_version["version"] = "9.9"
            path = self.write_yaml(directory, wrong_version)
            with self.assertRaisesRegex(ValueError, r"version must be '0.1'"):
                self.load_ledger(path)

            wrong_status = dict(ledger)
            wrong_status["status"] = "unknown"
            path = self.write_yaml(directory, wrong_status)
            with self.assertRaisesRegex(ValueError, "status must be one of"):
                self.load_ledger(path)

            wrong_release = dict(ledger)
            wrong_release["release"] = "v0.1.0-rc0"
            path = self.write_yaml(directory, wrong_release)
            with self.assertRaisesRegex(ValueError, "canonical v0.1.x"):
                self.load_ledger(path)

            wrong_root = dict(ledger)
            wrong_root["release_history_root"] = "abc"
            path = self.write_yaml(directory, wrong_root)
            with self.assertRaisesRegex(ValueError, "full lowercase Git commit ID"):
                self.load_ledger(path)

            extra = dict(ledger)
            extra["extra"] = True
            path = self.write_yaml(directory, extra)
            with self.assertRaisesRegex(ValueError, "keys must be"):
                self.load_ledger(path)

            legacy_item_fields = deepcopy(ledger)
            legacy_items = legacy_item_fields["items"]
            self.assertIsInstance(legacy_items, list)
            legacy_item = legacy_items[0]
            self.assertIsInstance(legacy_item, dict)
            legacy_item["source_item"] = 1
            legacy_item["title"] = "Duplicated source-owned title"
            path = self.write_yaml(directory, legacy_item_fields)
            with self.assertRaisesRegex(ValueError, r"items\[0\] keys must be"):
                self.load_ledger(path)

    def test_traceability_rejects_wrong_version_type_and_extra_field(self) -> None:
        traceability = load_yaml(REPO_ROOT / repository.DEFAULT_TRACEABILITY_PATH)
        self.assertIsInstance(traceability, dict)

        with tempfile.TemporaryDirectory() as directory:
            wrong_version = dict(traceability)
            wrong_version["version"] = "9.9"
            path = self.write_yaml(directory, wrong_version)
            with self.assertRaisesRegex(ValueError, "traceability.version"):
                traceability_codec.decode_report(load_yaml(path))

            wrong_status = dict(traceability)
            wrong_status["status"] = "unknown"
            path = self.write_yaml(directory, wrong_status)
            with self.assertRaisesRegex(ValueError, "traceability.status"):
                traceability_codec.decode_report(load_yaml(path))

            wrong_release = dict(traceability)
            wrong_release["release"] = "v0.1.0-rc0"
            path = self.write_yaml(directory, wrong_release)
            with self.assertRaisesRegex(ValueError, "canonical v0.1.x"):
                traceability_codec.decode_report(load_yaml(path))

            wrong_root = dict(traceability)
            wrong_root["release_history_root"] = "abc"
            path = self.write_yaml(directory, wrong_root)
            with self.assertRaisesRegex(ValueError, "full lowercase Git commit ID"):
                traceability_codec.decode_report(load_yaml(path))

            wrong_type = dict(traceability)
            wrong_type["summary"] = []
            path = self.write_yaml(directory, wrong_type)
            with self.assertRaisesRegex(
                TypeError, "traceability.summary must be a mapping"
            ):
                traceability_codec.decode_report(load_yaml(path))

            extra = dict(traceability)
            extra["extra"] = True
            path = self.write_yaml(directory, extra)
            with self.assertRaisesRegex(ValueError, "invalid keys"):
                traceability_codec.decode_report(load_yaml(path))

    def test_schema_narrowing_distinguishes_type_and_value_errors(self) -> None:
        wrong_type_cases = (
            (conformance_schema.expect_mapping, []),
            (conformance_schema.expect_list, {}),
            (conformance_schema.expect_str, 1),
            (conformance_schema.expect_bool, 1),
            (open_items.expect_mapping, []),
            (open_items.expect_list, {}),
            (open_items.expect_str, 1),
            (open_items.expect_int, True),
            (signoff_codec.parse_fraction, []),
        )
        for narrow, value in wrong_type_cases:
            with (
                self.subTest(narrow=narrow.__name__, value=value),
                self.assertRaises(TypeError),
            ):
                narrow(value, "field")

        invalid_value_cases: tuple[
            tuple[Callable[[YamlValue, str], object], YamlValue], ...
        ] = (
            (conformance_schema.expect_mapping, {1: "value"}),
            (conformance_schema.expect_str, ""),
            (open_items.expect_mapping, {1: "value"}),
            (signoff_codec.parse_fraction, "1/x"),
        )
        for narrow, value in invalid_value_cases:
            with (
                self.subTest(narrow=narrow.__name__, value=value),
                self.assertRaises(ValueError),
            ):
                narrow(value, "field")

    def test_schema_clis_normalize_type_errors_to_scoped_failures(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            malformed = self.write_yaml(directory, [])
            cases = (
                (
                    "validate_conformance_schema.py",
                    ["--repo-root", directory, "--manifest", str(malformed)],
                    "conformance schema validation failed",
                ),
                (
                    "validate_open_items.py",
                    ["--repo-root", directory, "--ledger", str(malformed)],
                    "OPEN ledger validation failed",
                ),
            )
            for script, arguments, message in cases:
                with self.subTest(script=script):
                    completed = subprocess.run(
                        [sys.executable, str(TOOLS_DIR / script), *arguments],
                        check=False,
                        capture_output=True,
                        text=True,
                    )

                self.assertEqual(completed.returncode, 1)
                self.assertIn(message, completed.stderr)

    def test_traceability_rejects_nested_unknown_fields(self) -> None:
        traceability = load_yaml(REPO_ROOT / repository.DEFAULT_TRACEABILITY_PATH)
        self.assertIsInstance(traceability, dict)

        mutations: list[YamlValue] = []
        for path in (
            ("sources",),
            ("summary",),
            ("coverage", 0),
            ("coverage", 0, "asserted_in", 0),
            ("optional_probe_summary", "optional_suites", 0),
            ("optional_probe_coverage", 0),
        ):
            mutation = deepcopy(traceability)
            target: object = mutation
            for key in path:
                if isinstance(key, int):
                    self.assertIsInstance(target, list)
                    target = target[key]
                else:
                    self.assertIsInstance(target, dict)
                    target = target[key]
            self.assertIsInstance(target, dict)
            target["extra"] = True
            mutations.append(mutation)

        with tempfile.TemporaryDirectory() as directory:
            for mutation in mutations:
                with self.subTest(mutation=mutation):
                    path = self.write_yaml(directory, mutation)
                    with self.assertRaisesRegex(ValueError, "unsupported"):
                        traceability_codec.decode_report(load_yaml(path))

    def test_traceability_rejects_wrong_optional_extension_mode(self) -> None:
        traceability = load_yaml(REPO_ROOT / repository.DEFAULT_TRACEABILITY_PATH)
        self.assertIsInstance(traceability, dict)
        mutation = deepcopy(traceability)
        extension_summary = mutation["optional_extension_summary"]
        self.assertIsInstance(extension_summary, dict)
        extension_summary["mode"] = "blocking"

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_yaml(directory, mutation)
            with self.assertRaisesRegex(ValueError, "expected non_blocking_evidence"):
                traceability_codec.decode_report(load_yaml(path))

    def test_spec_release_signoff_rejects_implementation_fields(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = self.write_yaml(
                directory,
                {key: "placeholder" for key in signoff_codec.SPEC_RELEASE_SIGNOFF_KEYS}
                | {
                    "specification": {
                        "release": "v0.1.0",
                        "commit": "abc",
                    },
                    "implementation": {
                        "name": "eglurc",
                        "version": "0.1.0",
                        "commit": "def",
                    },
                },
            )

            with self.assertRaisesRegex(ValueError, "unsupported"):
                signoff_codec.decode_spec_package_signoff(load_yaml(path))

    def test_spec_release_signoff_requires_canonical_bundle_digest(self) -> None:
        signoff: dict[str, YamlValue] = {
            key: "placeholder" for key in signoff_codec.SPEC_RELEASE_SIGNOFF_KEYS
        }
        signoff["specification"] = {
            "release": "v0.1.0",
            "commit": "a" * 40,
        }
        signoff["conformance_bundle_digest"] = "sha256:ABC"

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_yaml(directory, signoff)
            with self.assertRaisesRegex(ValueError, "64 lowercase hex"):
                signoff_codec.decode_spec_package_signoff(load_yaml(path))

    def test_static_gate_data_addresses_exact_manifest_and_suite_bytes(self) -> None:
        data = repository.load_snapshot(REPO_ROOT)
        manifest_path = REPO_ROOT / repository.DEFAULT_MANIFEST_PATH
        manifest = conformance_schema.parse_manifest(manifest_path)
        suite_bytes = {
            suite.file: (REPO_ROOT / manifest.root / suite.file).read_bytes()
            for suite in manifest.suites
        }
        expected = suite_artifact.conformance_bundle_digest(
            manifest_path.read_bytes(),
            suite_bytes,
        )
        self.assertEqual(data.conformance_bundle_digest, expected)

        first_path = min(suite_bytes)
        mutated = {
            **suite_bytes,
            first_path: suite_bytes[first_path] + b"\n# changed\n",
        }
        self.assertNotEqual(
            suite_artifact.conformance_bundle_digest(
                manifest_path.read_bytes(),
                mutated,
            ),
            expected,
        )

    def test_specification_identity_rejects_non_v01_release_labels(self) -> None:
        for release in (
            "banana-release",
            "v0.2.0",
            "v0.1.0-rcN",
            "v0.1.00",
            "v0.1.0-rc0",
        ):
            with (
                self.subTest(release=release),
                self.assertRaisesRegex(ValueError, "v0.1.x release"),
            ):
                signoff_codec.decode_specification(
                    {"release": release, "commit": "abc"},
                    "specification",
                )
        for release in ("v0.1.0", "v0.1.12", "v0.1.0-rc1"):
            with self.subTest(release=release):
                identity = signoff_codec.decode_specification(
                    {"release": release, "commit": "abc"},
                    "specification",
                )
                self.assertEqual(identity.release, release)

    def test_spec_release_signoff_binds_inventory_and_excludes_implementation(
        self,
    ) -> None:
        data = self.release_fixture()
        released_traceability = replace(data.traceability, status="released")
        released = replace(
            data,
            inventory=replace(data.inventory, status="released"),
            manifest=replace(data.manifest, status="released"),
            traceability=released_traceability,
            ledger=replace(data.ledger, status="released"),
        )
        summary = released.traceability.summary
        commit = "a" * 40
        signoff: dict[str, YamlValue] = {
            "kind": "spec_package_release",
            "specification": {"release": "v0.1.0", "commit": commit},
            "coverage_scope": traceability_model.COVERAGE_SCOPE,
            "artifact_inventory": str(repository.DEFAULT_INVENTORY_PATH),
            "manifest": str(repository.DEFAULT_MANIFEST_PATH),
            "conformance_bundle_digest": released.conformance_bundle_digest,
            "traceability": str(repository.DEFAULT_TRACEABILITY_PATH),
            "open_items_ledger": str(repository.DEFAULT_LEDGER_PATH),
            "suite_definitions": "VALID",
            "hard_required_gap_count": summary.hard_required_gap_count,
            "conditional_required_asserted_count": (
                f"{summary.conditional_required_asserted_count}/"
                f"{summary.conditional_required_code_count}"
            ),
            "optional_suites": "non-blocking",
            "optional_probe_optional_emit_if_present_asserted_count": (
                summary.optional_probe_optional_emit_if_present_asserted_count
            ),
            "optional_probe_warning_active": (
                released.traceability.optional_probe_warning_active
            ),
            "optional_probe_warning_note": (
                "warning active"
                if released.traceability.optional_probe_warning_active
                else "n/a"
            ),
            "optional_probe_warning_threshold_min_asserted": (
                released.traceability.optional_probe_policy.warning_threshold_min_asserted
            ),
            "open_items_unclassified_count": released.ledger.summary[
                "unclassified_count"
            ],
            "open_items_baseline_must_close_open_count": released.ledger.summary[
                "baseline_must_close_open_count"
            ],
            "implementation_conformance": "not_evaluated",
            "verdict": "GO",
            "approver": "reviewer",
            "timestamp_utc": "2026-07-30T00:00:00Z",
        }

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_yaml(directory, signoff)
            self.assertEqual(
                self.validate_signoff(
                    released,
                    path,
                    commit,
                ),
                [],
            )

            signoff["implementation_conformance"] = "PASS"
            path = self.write_yaml(directory, signoff)
            problems = self.validate_signoff(
                released,
                path,
                commit,
            )

        self.assertTrue(
            any(
                "implementation_conformance is PASS, expected not_evaluated"
                in problem.message
                for problem in problems
            )
        )

        signoff["implementation_conformance"] = "not_evaluated"
        signoff["artifact_inventory"] = "README.md"
        with tempfile.TemporaryDirectory() as directory:
            path = self.write_yaml(directory, signoff)
            problems = self.validate_signoff(
                released,
                path,
                commit,
            )
        self.assertTrue(
            any(
                "signoff.artifact_inventory is README.md" in problem.message
                for problem in problems
            )
        )

        signoff["artifact_inventory"] = str(repository.DEFAULT_INVENTORY_PATH)
        signoff["conformance_bundle_digest"] = f"sha256:{'f' * 64}"
        with tempfile.TemporaryDirectory() as directory:
            path = self.write_yaml(directory, signoff)
            problems = self.validate_signoff(
                released,
                path,
                commit,
            )
        self.assertTrue(
            any(
                "signoff.conformance_bundle_digest is" in problem.message
                for problem in problems
            )
        )

        signoff["conformance_bundle_digest"] = released.conformance_bundle_digest
        specification = signoff["specification"]
        self.assertIsInstance(specification, dict)
        specification["release"] = "v0.1.1"
        with tempfile.TemporaryDirectory() as directory:
            path = self.write_yaml(directory, signoff)
            problems = self.validate_signoff(
                released,
                path,
                commit,
            )
        self.assertTrue(
            any(
                "signoff.specification.release is v0.1.1, expected v0.1.0"
                in problem.message
                for problem in problems
            )
        )

    def test_artifact_release_state_must_match_and_be_released_for_signoff(
        self,
    ) -> None:
        data = self.release_fixture()

        self.assertEqual(
            release_state.validate(data.release_carriers()),
            [],
        )
        self.assertTrue(
            release_state.validate(
                data.release_carriers(),
                require_released=True,
            )
        )

        mismatched = replace(
            data,
            inventory=replace(data.inventory, status="released"),
        )
        mismatch_problems = release_state.validate(mismatched.release_carriers())
        self.assertTrue(
            any(
                "artifact status mismatch" in item.message for item in mismatch_problems
            )
        )

        identity_mismatch = replace(
            data,
            manifest=replace(data.manifest, release="v0.1.1"),
        )
        identity_problems = release_state.validate(identity_mismatch.release_carriers())
        self.assertTrue(
            any(
                "artifact release identity mismatch" in item.message
                for item in identity_problems
            )
        )

        root_mismatch = replace(
            data,
            manifest=replace(data.manifest, release_history_root="a" * 40),
        )
        root_problems = release_state.validate(root_mismatch.release_carriers())
        self.assertTrue(
            any(
                "artifact release history root mismatch" in item.message
                for item in root_problems
            )
        )

        released_traceability = replace(data.traceability, status="released")
        released = replace(
            data,
            inventory=replace(data.inventory, status="released"),
            manifest=replace(data.manifest, status="released"),
            traceability=released_traceability,
            ledger=replace(data.ledger, status="released"),
        )
        self.assertEqual(
            release_state.validate(
                released.release_carriers(),
                require_released=True,
            ),
            [],
        )


if __name__ == "__main__":
    unittest.main()

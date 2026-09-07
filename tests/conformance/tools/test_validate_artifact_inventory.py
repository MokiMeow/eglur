"""Mutation tests for the authoritative artifact inventory."""

import shutil
import subprocess
import tempfile
import unittest
from copy import deepcopy
from pathlib import Path
from typing import cast

import yaml

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


from eglur_conformance.spec_release import artifact_inventory
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue, load_yaml


class ArtifactInventoryTests(unittest.TestCase):
    """Require complete and non-overlapping authoritative classification."""

    def load_inventory(self, path: Path) -> artifact_inventory.ArtifactInventory:
        """Decode one inventory fixture."""
        return artifact_inventory.decode_inventory(load_yaml(path), str(path))

    def validate_classification(
        self,
        repo_root: Path,
        inventory: artifact_inventory.ArtifactInventory,
    ) -> list[ValidationProblem]:
        """Validate inventory policy against artifacts in one checkout."""
        candidates = artifact_inventory.discover_authoritative_artifacts(repo_root)
        return artifact_inventory.validate_inventory(inventory, candidates)

    def load_canonical_mapping(self) -> dict[str, YamlValue]:
        """Load a mutable copy of the canonical inventory mapping."""
        value = load_yaml(REPO_ROOT / artifact_inventory.DEFAULT_INVENTORY_PATH)
        self.assertIsInstance(value, dict)
        self.assertTrue(all(isinstance(key, str) for key in value))
        return deepcopy(cast(dict[str, YamlValue], value))

    def write_inventory(self, directory: str, value: object) -> Path:
        """Write one synthetic inventory."""
        path = Path(directory) / "inventory.yaml"
        rendered = yaml.safe_dump(value, sort_keys=False)
        if not isinstance(rendered, str):
            raise TypeError("synthetic YAML serialization must return text")
        path.write_text(rendered, encoding="utf-8")
        return path

    def test_canonical_inventory_classifies_every_artifact_once(self) -> None:
        inventory = self.load_inventory(
            REPO_ROOT / artifact_inventory.DEFAULT_INVENTORY_PATH
        )

        self.assertEqual(
            self.validate_classification(REPO_ROOT, inventory),
            [],
        )

    def test_closed_schema_rejects_version_status_and_unknown_keys(self) -> None:
        base = self.load_canonical_mapping()
        mutations = (
            ({**base, "version": "9.9"}, "version must be"),
            (
                {**base, "release_history_root": "abc"},
                "full lowercase Git commit ID",
            ),
            ({**base, "release": "v0.1.00"}, "canonical v0.1.x"),
            ({**base, "status": "unknown"}, "status must be one of"),
            ({**base, "extra": True}, "unsupported"),
        )

        with tempfile.TemporaryDirectory() as directory:
            for mutation, message in mutations:
                with self.subTest(message=message):
                    path = self.write_inventory(directory, mutation)
                    with self.assertRaisesRegex(ValueError, message):
                        self.load_inventory(path)

    def test_rejects_unsupported_classification(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        artifacts[0]["provenance"] = "generated"

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            with self.assertRaisesRegex(ValueError, "unsupported classification"):
                self.load_inventory(path)

    def test_reports_unclassified_and_multiply_classified_artifacts(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        artifacts.pop(0)
        artifacts.append(
            {
                "selector": "docs/spec/SPEC.md",
                "role": "normative_spec",
                "provenance": "authored",
                "status_source": "inventory",
            }
        )
        artifacts.append(
            {
                "selector": "docs/spec/SPEC*.md",
                "role": "normative_spec",
                "provenance": "authored",
                "status_source": "inventory",
            }
        )

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            inventory = self.load_inventory(path)
            messages = [
                problem.message
                for problem in self.validate_classification(
                    REPO_ROOT,
                    inventory,
                )
            ]

        self.assertTrue(
            any(
                "unclassified artifact: docs/spec/CONFORMANCE.md" in item
                for item in messages
            )
        )
        self.assertTrue(
            any(
                "docs/spec/SPEC.md is classified by multiple" in item
                for item in messages
            )
        )

    def test_rejects_wrong_established_classification(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        manifest = next(
            item
            for item in artifacts
            if item["selector"] == "tests/conformance/manifest.yaml"
        )
        manifest["role"] = "release_control"

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            inventory = self.load_inventory(path)
            problems = self.validate_classification(
                REPO_ROOT,
                inventory,
            )

        self.assertTrue(
            any(
                "tests/conformance/manifest.yaml classification" in problem.message
                for problem in problems
            )
        )

    def test_requires_open_item_sources_to_be_release_control(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        open_items = next(
            item
            for item in artifacts
            if item["selector"] == "docs/spec/open-items/*.md"
        )
        open_items["role"] = "normative_spec"

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            inventory = self.load_inventory(path)
            problems = self.validate_classification(REPO_ROOT, inventory)

        self.assertTrue(
            any(
                "docs/spec/open-items/language.md classification" in problem.message
                for problem in problems
            )
        )

    def test_registry_cache_probe_inherits_inventory_status(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        probe = next(
            item
            for item in artifacts
            if item["selector"] == "tests/conformance/probes/registry_cache.yaml"
        )
        probe["status_source"] = "manifest"

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            inventory = self.load_inventory(path)
            problems = self.validate_classification(REPO_ROOT, inventory)

        self.assertTrue(
            any(
                "tests/conformance/probes/registry_cache.yaml classification"
                in problem.message
                for problem in problems
            )
        )

    def test_unregistered_probe_definition_is_not_admitted_by_inventory(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        probe = next(
            item
            for item in artifacts
            if item["selector"] == "tests/conformance/probes/registry_cache.yaml"
        )
        probe["selector"] = "tests/conformance/probes/*.yaml"
        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            inventory = self.load_inventory(path)
        candidates = set(artifact_inventory.discover_authoritative_artifacts(REPO_ROOT))
        candidates.add("tests/conformance/probes/unvalidated.yaml")

        problems = artifact_inventory.validate_inventory(inventory, candidates)

        self.assertTrue(
            any(
                problem.message == "unregistered implementation probe: "
                "tests/conformance/probes/unvalidated.yaml"
                for problem in problems
            )
        )
        self.assertTrue(
            any(
                "registered implementation probe "
                "tests/conformance/probes/registry_cache.yaml must use an exact selector"
                in problem.message
                for problem in problems
            )
        )

    def test_requires_status_and_generated_validator_registration(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        contract = next(
            item
            for item in artifacts
            if item["selector"] == "docs/spec/errors/diagnostic-contracts.yaml"
        )
        contract.update(
            {
                "role": "generated_evidence",
                "provenance": "generated",
                "status_source": "self",
            }
        )

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            inventory = self.load_inventory(path)
            messages = [
                problem.message
                for problem in self.validate_classification(
                    REPO_ROOT,
                    inventory,
                )
            ]

        self.assertTrue(
            any("has no registered status validator" in item for item in messages)
        )
        self.assertTrue(
            any("has no registered parity validator" in item for item in messages)
        )

    def test_rejects_selector_outside_authoritative_roots(self) -> None:
        value = self.load_canonical_mapping()
        artifacts = value["artifacts"]
        self.assertIsInstance(artifacts, list)
        artifacts.append(
            {
                "selector": "README.md",
                "role": "normative_spec",
                "provenance": "authored",
                "status_source": "inventory",
            }
        )

        with tempfile.TemporaryDirectory() as directory:
            path = self.write_inventory(directory, value)
            inventory = self.load_inventory(path)
            problems = self.validate_classification(
                REPO_ROOT,
                inventory,
            )

        self.assertTrue(
            any("must start with one of" in problem.message for problem in problems)
        )

    def test_rejects_untracked_yml_and_tracked_symlink_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            (repo_root / "docs/spec").mkdir(parents=True)
            (repo_root / "tests/conformance").mkdir(parents=True)
            (repo_root / "docs/spec/SPEC.md").write_text(
                "# Spec\n",
                encoding="utf-8",
            )
            (repo_root / "docs/spec/artifact-inventory-v0.1.yaml").write_text(
                "version: '0.1'\n"
                f"release_history_root: {'a' * 40}\n"
                "release: v0.1.0\n"
                "status: draft\n"
                "artifacts: []\n",
                encoding="utf-8",
            )
            subprocess.run(["git", "init", "-q"], cwd=repo_root, check=True)
            subprocess.run(
                ["git", "config", "user.email", "inventory@example.invalid"],
                cwd=repo_root,
                check=True,
            )
            subprocess.run(
                ["git", "config", "user.name", "Inventory Test"],
                cwd=repo_root,
                check=True,
            )
            subprocess.run(["git", "add", "."], cwd=repo_root, check=True)
            subprocess.run(
                ["git", "commit", "-qm", "fixture"],
                cwd=repo_root,
                check=True,
            )

            inventory = artifact_inventory.ArtifactInventory(
                version="0.1",
                release_history_root="a" * 40,
                release="v0.1.0",
                status="draft",
                artifacts=(
                    artifact_inventory.ArtifactRule(
                        selector="docs/spec/*.md",
                        role="normative_spec",
                        provenance="authored",
                        status_source="inventory",
                    ),
                    artifact_inventory.ArtifactRule(
                        selector="docs/spec/artifact-inventory-v0.1.yaml",
                        role="release_control",
                        provenance="authored",
                        status_source="self",
                    ),
                ),
            )
            self.assertEqual(
                artifact_inventory.validate_checkout_binding(repo_root),
                [],
            )
            self.assertEqual(
                self.validate_classification(repo_root, inventory),
                [],
            )

            spec_path = repo_root / "docs/spec/SPEC.md"
            subprocess.run(
                ["git", "update-index", "--skip-worktree", "docs/spec/SPEC.md"],
                cwd=repo_root,
                check=True,
            )
            spec_path.write_text("# Hidden mutation\n", encoding="utf-8")
            messages = [
                problem.message
                for problem in artifact_inventory.validate_checkout_binding(repo_root)
            ]
            self.assertTrue(
                any(
                    "working-tree bytes for authoritative artifact "
                    "docs/spec/SPEC.md do not match the Git index" in item
                    for item in messages
                )
            )
            spec_path.write_text("# Spec\n", encoding="utf-8")

            with tempfile.TemporaryDirectory() as replacement_directory:
                replacement = Path(replacement_directory) / "replacement.md"
                replacement.write_text("# Replacement\n", encoding="utf-8")
                spec_path.unlink()
                spec_path.symlink_to(replacement)
                messages = [
                    problem.message
                    for problem in (
                        artifact_inventory.validate_checkout_binding(repo_root)
                    )
                ]
                self.assertTrue(
                    any(
                        "working-tree authoritative artifact docs/spec/SPEC.md "
                        "must be a regular file" in item
                        for item in messages
                    )
                )
                spec_path.unlink()

            spec_path.write_text("# Spec\n", encoding="utf-8")
            subprocess.run(
                ["git", "update-index", "--no-skip-worktree", "docs/spec/SPEC.md"],
                cwd=repo_root,
                check=True,
            )

            spec_directory = repo_root / "docs/spec"
            with tempfile.TemporaryDirectory() as external_directory:
                external_spec = Path(external_directory) / "spec"
                shutil.copytree(spec_directory, external_spec)
                shutil.rmtree(spec_directory)
                spec_directory.symlink_to(external_spec, target_is_directory=True)
                messages = [
                    problem.message
                    for problem in (
                        artifact_inventory.validate_checkout_binding(repo_root)
                    )
                ]
                self.assertTrue(
                    any(
                        "authoritative artifact ancestor docs/spec "
                        "must not be a symlink" in item
                        for item in messages
                    )
                )
                spec_directory.unlink()
                shutil.copytree(external_spec, spec_directory)

            (repo_root / "docs/spec/untracked.md").write_text(
                "# Untracked\n",
                encoding="utf-8",
            )
            (repo_root / "docs/spec/hidden.yml").write_text(
                "value: hidden\n",
                encoding="utf-8",
            )
            (repo_root / "docs/spec/uppercase.YAML").write_text(
                "value: uppercase\n",
                encoding="utf-8",
            )
            (repo_root / "docs/spec/mixed.YmL").write_text(
                "value: mixed\n",
                encoding="utf-8",
            )
            subprocess.run(
                [
                    "git",
                    "add",
                    "docs/spec/uppercase.YAML",
                    "docs/spec/mixed.YmL",
                ],
                cwd=repo_root,
                check=True,
            )
            with tempfile.TemporaryDirectory() as external_directory:
                external = Path(external_directory) / "external.md"
                external.write_text("# External\n", encoding="utf-8")
                symlink = repo_root / "docs/spec/external.md"
                symlink.symlink_to(external)
                subprocess.run(
                    ["git", "add", "docs/spec/external.md"],
                    cwd=repo_root,
                    check=True,
                )

                problems = artifact_inventory.validate_checkout_binding(repo_root)
                problems.extend(self.validate_classification(repo_root, inventory))
                messages = [problem.message for problem in problems]

                self.assertTrue(
                    any(
                        "not tracked in the Git index: docs/spec/untracked.md" in item
                        for item in messages
                    )
                )
                self.assertTrue(
                    any(
                        "exact .yaml extension: docs/spec/hidden.yml" in item
                        for item in messages
                    )
                )
                self.assertTrue(
                    any(
                        "exact .yaml extension: docs/spec/uppercase.YAML" in item
                        for item in messages
                    )
                )
                self.assertTrue(
                    any(
                        "exact .yaml extension: docs/spec/mixed.YmL" in item
                        for item in messages
                    )
                )
                self.assertTrue(
                    any(
                        "docs/spec/external.md must be a regular tracked file" in item
                        for item in messages
                    )
                )


if __name__ == "__main__":
    unittest.main()

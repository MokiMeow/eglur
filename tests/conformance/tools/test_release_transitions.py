"""Adversarial tests for specification release identity transitions."""

import subprocess
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path, PurePosixPath
from unittest.mock import patch

import yaml

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


from eglur_conformance import release_identity
from eglur_conformance.spec_release import (
    artifact_inventory,
    gate,
    git_repository,
    history,
    open_items,
    release_state,
    repository,
)
from eglur_conformance.suite import model as suite_model
from eglur_conformance.traceability import repository as traceability_repository
from eglur_conformance.validation import ValidationProblem


def yaml_text(value: object) -> str:
    """Serialize one synthetic YAML document as text."""
    rendered = yaml.safe_dump(value, sort_keys=False)
    if not isinstance(rendered, str):
        raise TypeError("synthetic YAML serialization must return text")
    return rendered


class ReleaseTransitionTests(unittest.TestCase):
    """Require monotonic identities and one-way draft-to-release state."""

    def state(
        self,
        release: str,
        status: str,
        history_root: str = "a" * 40,
    ) -> release_state.ArtifactReleaseState:
        """Build one narrowed release state."""
        return release_state.ArtifactReleaseState(
            history_root=history_root,
            release=release_identity.parse_release_label(release, "test.release"),
            status=status,
        )

    def gate_data(
        self,
        history_root: str,
        release: str,
        status: str,
    ) -> repository.SpecPackageSnapshot:
        """Build the release fields needed by historical validation."""
        return repository.SpecPackageSnapshot(
            inventory=artifact_inventory.ArtifactInventory(
                version="0.1",
                release_history_root=history_root,
                release=release,
                status=status,
                artifacts=(),
            ),
            manifest=suite_model.Manifest(
                version="0.1",
                release_history_root=history_root,
                release=release,
                status=status,
                root="tests/conformance",
                suites=(),
            ),
            conformance_bundle_digest=f"sha256:{'0' * 64}",
            traceability=replace(
                traceability_repository.read_report(REPO_ROOT),
                release_history_root=history_root,
                release=release_identity.parse_release_label(
                    release,
                    "test.traceability.release",
                ),
                status=status,
            ),
            ledger=open_items.Ledger(
                version="0.1",
                release_history_root=history_root,
                release=release,
                status=status,
                purpose="test",
                classification_keys=(),
                sources=(),
                items=(),
                summary={},
            ),
        )

    def validate_history(
        self,
        repo_root: Path,
        snapshot: repository.SpecPackageSnapshot,
    ) -> list[ValidationProblem]:
        """Validate one snapshot against its repository history."""
        state, problems = release_state.agree(snapshot.release_carriers())
        self.assertEqual(problems, [])
        self.assertIsNotNone(state)
        if state is None:
            return []
        return history.validate(
            git_repository.GitRepository(repo_root),
            state,
        )

    def write_release_artifacts(
        self,
        repo_root: Path,
        *,
        history_root: str | None,
        release: str | None,
        status: str,
        marker: str | None = None,
    ) -> None:
        """Write the four historical state carriers used by the gate."""
        for path in repository.RELEASE_STATE_PATHS.values():
            target = repo_root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            document: dict[str, str] = {
                "version": "0.1",
                "status": status,
            }
            if release is not None:
                if history_root is None:
                    raise ValueError("release state requires a history root")
                document["release_history_root"] = history_root
                document["release"] = release
            if marker is not None:
                document["marker"] = marker
            target.write_text(
                yaml_text(document),
                encoding="utf-8",
            )

    def git(self, repo_root: Path, *arguments: str) -> None:
        """Run one quiet Git fixture command."""
        subprocess.run(
            ["git", *arguments],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )

    def git_output(self, repo_root: Path, *arguments: str) -> str:
        """Return stripped output from one Git fixture command."""
        return subprocess.run(
            ["git", *arguments],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

    def initialize_legacy_history(self, repo_root: Path) -> str:
        """Create one legacy root commit and return its full ID."""
        self.git(repo_root, "init", "-q")
        self.git(
            repo_root,
            "config",
            "user.email",
            "release@example.invalid",
        )
        self.git(repo_root, "config", "user.name", "Release Test")
        self.write_release_artifacts(
            repo_root,
            history_root=None,
            release=None,
            status="draft",
        )
        self.git(repo_root, "add", ".")
        self.git(repo_root, "commit", "-qm", "legacy")
        return self.git_output(repo_root, "rev-parse", "HEAD")

    def commit_state(
        self,
        repo_root: Path,
        history_root: str,
        release: str,
        status: str,
        message: str,
    ) -> str:
        """Write and commit one complete release state."""
        self.write_release_artifacts(
            repo_root,
            history_root=history_root,
            release=release,
            status=status,
        )
        self.git(repo_root, "add", ".")
        self.git(repo_root, "commit", "-qm", message)
        return self.git_output(repo_root, "rev-parse", "HEAD")

    def test_release_label_ordering_is_canonical(self) -> None:
        rc1 = release_identity.parse_release_label("v0.1.0-rc1", "release")
        rc2 = release_identity.parse_release_label("v0.1.0-rc2", "release")
        final = release_identity.parse_release_label("v0.1.0", "release")
        next_rc = release_identity.parse_release_label(
            "v0.1.1-rc1",
            "release",
        )

        self.assertTrue(rc2.is_later_than(rc1))
        self.assertTrue(final.is_later_than(rc2))
        self.assertTrue(next_rc.is_later_than(final))

        abandoned_rc = self.state("v0.1.0-rc1", "draft")
        next_patch_rc = self.state("v0.1.1-rc1", "draft")
        self.assertEqual(
            release_state.validate_transition(
                abandoned_rc,
                next_patch_rc,
                "abandon candidate",
            ),
            [],
        )

    def test_repository_root_bootstraps_without_a_self_hash(self) -> None:
        for status in ("draft", "released"):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                self.git(root, "init", "-q")
                self.git(root, "config", "user.email", "release@example.invalid")
                self.git(root, "config", "user.name", "Release Test")
                self.commit_state(root, "repository-root", "v0.1.0", status, "initial")
                self.assertEqual(
                    self.git_output(root, "rev-list", "--count", "HEAD"), "1"
                )
                snapshot = self.gate_data("repository-root", "v0.1.0", status)
                self.assertEqual(self.validate_history(root, snapshot), [])

    def test_repository_root_preserves_later_transitions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.git(root, "init", "-q")
            self.git(root, "config", "user.email", "release@example.invalid")
            self.git(root, "config", "user.name", "Release Test")
            self.commit_state(root, "repository-root", "v0.1.0", "released", "initial")
            (root / "note.txt").write_text("editorial change\n", encoding="utf-8")
            self.git(root, "add", ".")
            snapshot = self.gate_data("repository-root", "v0.1.0", "released")
            problems = self.validate_history(root, snapshot)
            self.assertTrue(any("already released" in str(p) for p in problems))
            self.commit_state(root, "repository-root", "v0.1.1", "draft", "next draft")
            self.assertEqual(
                self.validate_history(
                    root, self.gate_data("repository-root", "v0.1.1", "draft")
                ),
                [],
            )

    def test_repository_root_cannot_skip_a_stateless_initial_commit(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.initialize_legacy_history(root)
            self.commit_state(root, "repository-root", "v0.1.0", "draft", "late state")
            problems = self.validate_history(
                root, self.gate_data("repository-root", "v0.1.0", "draft")
            )
            self.assertTrue(any("no release state" in str(p) for p in problems))

    def test_batch_blob_reader_fails_closed_on_malformed_output(self) -> None:
        commit = "a" * 40
        path = PurePosixPath("artifact.yaml")
        query = git_repository.BlobQuery(commit=commit, path=path)
        git = git_repository.GitRepository(Path("."))
        object_id = "b" * 40
        object_spec = f"{commit}:{path}"

        missing = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=f"{object_spec} missing\n".encode(),
            stderr=b"",
        )
        with patch.object(
            git_repository.subprocess,
            "run",
            return_value=missing,
        ):
            self.assertEqual(
                git.read_text_blobs([query]),
                {query: None},
            )

        malformed_outputs = (
            b"",
            f"{object_id} tree 0\n\n".encode(),
            f"{object_id} blob 4\nabc\n".encode(),
            f"{object_id} blob 1\n".encode() + b"\xff\n",
        )
        for stdout in malformed_outputs:
            with (
                self.subTest(stdout=stdout),
                patch.object(
                    git_repository.subprocess,
                    "run",
                    return_value=subprocess.CompletedProcess(
                        args=[],
                        returncode=0,
                        stdout=stdout,
                        stderr=b"",
                    ),
                ),
                self.assertRaises(ValueError),
            ):
                git.read_text_blobs([query])

    def test_batch_blob_reader_deduplicates_queries_in_one_subprocess(self) -> None:
        first = git_repository.BlobQuery(
            commit="a" * 40,
            path=PurePosixPath("first.yaml"),
        )
        second = git_repository.BlobQuery(
            commit="b" * 40,
            path=PurePosixPath("second.yaml"),
        )
        completed = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=(f"{'c' * 40} blob 3\none\n{'d' * 40} blob 3\ntwo\n").encode(),
            stderr=b"",
        )

        with patch.object(
            git_repository.subprocess,
            "run",
            return_value=completed,
        ) as run:
            blobs = git_repository.GitRepository(Path(".")).read_text_blobs(
                [first, second, first]
            )

        self.assertEqual(blobs, {first: "one", second: "two"})
        run.assert_called_once()
        self.assertEqual(
            run.call_args.kwargs["input"],
            f"{first.object_spec}\n{second.object_spec}\n".encode(),
        )

    def test_pure_transition_contract(self) -> None:
        draft = self.state("v0.1.0", "draft")
        released = self.state("v0.1.0", "released")
        next_draft = self.state("v0.1.1", "draft")
        next_released = self.state("v0.1.1", "released")

        self.assertEqual(
            release_state.validate_transition(
                None,
                draft,
                "bootstrap",
            ),
            [],
        )
        self.assertEqual(
            release_state.validate_transition(
                draft,
                released,
                "release",
            ),
            [],
        )
        self.assertEqual(
            release_state.validate_transition(
                released,
                next_draft,
                "next",
            ),
            [],
        )
        self.assertTrue(
            release_state.validate_transition(
                None,
                released,
                "bootstrap",
            )
        )
        self.assertTrue(
            release_state.validate_transition(
                released,
                released,
                "reuse",
            )
        )
        self.assertTrue(
            release_state.validate_transition(
                released,
                next_released,
                "skip-draft",
            )
        )
        self.assertTrue(
            release_state.validate_transition(
                next_draft,
                draft,
                "regression",
            )
        )

    def test_git_history_enforces_bootstrap_release_and_next_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            history_root = self.initialize_legacy_history(repo_root)

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.0",
                status="draft",
            )
            self.git(repo_root, "add", ".")
            draft = self.gate_data(history_root, "v0.1.0", "draft")
            self.assertEqual(
                self.validate_history(
                    repo_root,
                    draft,
                ),
                [],
            )
            self.git(repo_root, "commit", "-qm", "start release")
            self.assertEqual(
                self.validate_history(
                    repo_root,
                    draft,
                ),
                [],
            )

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.0",
                status="released",
            )
            self.git(repo_root, "add", ".")
            released = self.gate_data(
                history_root,
                "v0.1.0",
                "released",
            )
            self.assertEqual(
                self.validate_history(
                    repo_root,
                    released,
                ),
                [],
            )
            self.git(repo_root, "commit", "-qm", "release")
            self.assertEqual(
                self.validate_history(
                    repo_root,
                    released,
                ),
                [],
            )

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.0",
                status="released",
                marker="post-release mutation",
            )
            self.git(repo_root, "add", ".")
            reuse_problems = self.validate_history(
                repo_root,
                released,
            )
            self.assertTrue(
                any(
                    "was already released and cannot be reused" in problem.message
                    for problem in reuse_problems
                )
            )

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.1",
                status="draft",
            )
            self.git(repo_root, "add", ".")
            next_draft = self.gate_data(
                history_root,
                "v0.1.1",
                "draft",
            )
            self.assertEqual(
                self.validate_history(
                    repo_root,
                    next_draft,
                ),
                [],
            )

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.1",
                status="released",
            )
            self.git(repo_root, "add", ".")
            next_released = self.gate_data(
                history_root,
                "v0.1.1",
                "released",
            )
            direct_release_problems = self.validate_history(
                repo_root,
                next_released,
            )
            self.assertTrue(
                any(
                    "new release identity v0.1.1 must start in draft" in problem.message
                    for problem in direct_release_problems
                )
            )

    def test_prior_release_cannot_be_laundered_through_a_new_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            history_root = self.initialize_legacy_history(repo_root)
            self.commit_state(
                repo_root,
                history_root,
                "v0.1.0",
                "draft",
                "start release",
            )
            self.commit_state(
                repo_root,
                history_root,
                "v0.1.0",
                "released",
                "release",
            )

            self.write_release_artifacts(
                repo_root,
                history_root=None,
                release=None,
                status="draft",
            )
            self.git(repo_root, "add", ".")
            self.git(repo_root, "commit", "-qm", "strip state")
            replacement_root = self.git_output(repo_root, "rev-parse", "HEAD")

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.0",
                status="draft",
            )
            self.git(repo_root, "add", ".")
            same_root_problems = self.validate_history(
                repo_root,
                self.gate_data(history_root, "v0.1.0", "draft"),
            )
            self.assertTrue(
                any(
                    "has no release state" in problem.message
                    for problem in same_root_problems
                )
            )

            self.write_release_artifacts(
                repo_root,
                history_root=replacement_root,
                release="v0.1.0",
                status="draft",
            )
            self.git(repo_root, "add", ".")
            problems = self.validate_history(
                repo_root,
                self.gate_data(replacement_root, "v0.1.0", "draft"),
            )

            self.assertTrue(
                any(
                    "prior artifact_inventory release state" in problem.message
                    for problem in problems
                )
            )

            self.git(
                repo_root,
                "replace",
                history_root,
                replacement_root,
            )
            replacement_problems = self.validate_history(
                repo_root,
                self.gate_data(replacement_root, "v0.1.0", "draft"),
            )
            self.assertTrue(
                any(
                    "forbids Git replacement refs" in problem.message
                    for problem in replacement_problems
                )
            )

    def test_history_root_rejects_partial_state_in_any_carrier(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            self.initialize_legacy_history(repo_root)

            manifest_path = repo_root / repository.DEFAULT_MANIFEST_PATH
            manifest_path.write_text(
                yaml_text(
                    {
                        "version": "0.1",
                        "release_history_root": "a" * 40,
                        "release": "v0.1.9",
                        "status": "released",
                    }
                ),
                encoding="utf-8",
            )
            self.git(repo_root, "add", ".")
            self.git(repo_root, "commit", "-qm", "partial state")

            self.write_release_artifacts(
                repo_root,
                history_root=None,
                release=None,
                status="draft",
            )
            self.git(repo_root, "add", ".")
            self.git(repo_root, "commit", "-qm", "strip partial state")
            replacement_root = self.git_output(repo_root, "rev-parse", "HEAD")

            self.write_release_artifacts(
                repo_root,
                history_root=replacement_root,
                release="v0.1.0",
                status="draft",
            )
            self.git(repo_root, "add", ".")
            problems = self.validate_history(
                repo_root,
                self.gate_data(replacement_root, "v0.1.0", "draft"),
            )

            self.assertTrue(
                any(
                    "prior manifest release state" in problem.message
                    for problem in problems
                )
            )

    def test_released_identity_is_unique_across_merged_branches(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            history_root = self.initialize_legacy_history(repo_root)
            draft_commit = self.commit_state(
                repo_root,
                history_root,
                "v0.1.0",
                "draft",
                "start release",
            )

            self.git(repo_root, "switch", "-qc", "left")
            self.commit_state(
                repo_root,
                history_root,
                "v0.1.0",
                "released",
                "left release",
            )
            self.git(repo_root, "switch", "-qc", "right", draft_commit)
            self.commit_state(
                repo_root,
                history_root,
                "v0.1.0",
                "released",
                "right release",
            )
            self.git(repo_root, "switch", "-q", "left")
            self.git(repo_root, "merge", "--no-commit", "--no-ff", "right")
            clean_problems = gate.validate_clean_checkout(
                git_repository.GitRepository(repo_root)
            )
            self.assertTrue(
                any(
                    "MERGE_HEAD is present" in problem.message
                    for problem in clean_problems
                )
            )

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.1",
                status="draft",
            )
            self.git(repo_root, "add", ".")
            problems = self.validate_history(
                repo_root,
                self.gate_data(history_root, "v0.1.1", "draft"),
            )

            self.assertTrue(
                any(
                    "released identity v0.1.0 is bound to multiple commits"
                    in problem.message
                    for problem in problems
                )
            )

    def test_prospective_merge_checks_merge_head(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            history_root = self.initialize_legacy_history(repo_root)
            self.commit_state(
                repo_root,
                history_root,
                "v0.1.0",
                "draft",
                "start release",
            )
            base_branch = self.git_output(repo_root, "branch", "--show-current")

            self.git(repo_root, "switch", "-qc", "future")
            self.commit_state(
                repo_root,
                history_root,
                "v0.1.2",
                "draft",
                "future draft",
            )
            self.commit_state(
                repo_root,
                history_root,
                "v0.1.2",
                "released",
                "future release",
            )
            self.git(repo_root, "switch", "-q", base_branch)
            self.git(repo_root, "merge", "--no-commit", "--no-ff", "future")

            self.write_release_artifacts(
                repo_root,
                history_root=history_root,
                release="v0.1.1",
                status="draft",
            )
            self.git(repo_root, "add", ".")
            problems = self.validate_history(
                repo_root,
                self.gate_data(history_root, "v0.1.1", "draft"),
            )

            self.assertTrue(
                any(
                    "v0.1.1 must be later than v0.1.2" in problem.message
                    for problem in problems
                )
            )

    def initialize_repository_history(self, root: Path) -> str:
        self.git(root, "init", "-q")
        self.git(root, "config", "user.email", "release@example.invalid")
        self.git(root, "config", "user.name", "Release Test")
        return self.commit_state(root, "repository-root", "v0.1.0", "draft", "initial")

    def test_repository_origin_rejects_missing_partial_and_changed_state(self) -> None:
        for mutation in ("missing", "partial", "changed-origin"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                initial = self.initialize_repository_history(root)
                if mutation == "missing":
                    self.write_release_artifacts(
                        root, history_root=None, release=None, status="draft"
                    )
                elif mutation == "partial":
                    (root / repository.RELEASE_STATE_PATHS["manifest"]).unlink()
                else:
                    self.write_release_artifacts(
                        root, history_root=initial, release="v0.1.0", status="draft"
                    )
                self.git(root, "add", "-A")
                self.git(root, "commit", "-qm", mutation)
                self.commit_state(root, "repository-root", "v0.1.0", "draft", "restore")
                problems = self.validate_history(
                    root, self.gate_data("repository-root", "v0.1.0", "draft")
                )
                expected = {
                    "missing": "no release state",
                    "partial": "partial identity coverage",
                    "changed-origin": "release history root changed",
                }[mutation]
                self.assertTrue(any(expected in p.message for p in problems), problems)

    def test_repository_origin_rejects_multiple_roots_before_and_after_merge(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            initial = self.initialize_repository_history(root)
            self.git(root, "checkout", "--orphan", "unrelated")
            self.git(root, "commit", "-qm", "unrelated initial")
            self.git(root, "checkout", "-qb", "mainline", initial)
            self.git(
                root,
                "merge",
                "--allow-unrelated-histories",
                "--no-commit",
                "--no-ff",
                "unrelated",
            )
            snapshot = self.gate_data("repository-root", "v0.1.0", "draft")
            for prospective in (True, False):
                if not prospective:
                    self.git(root, "commit", "-qm", "merge unrelated")
                problems = self.validate_history(root, snapshot)
                self.assertTrue(
                    any(
                        "exactly one reachable Git root" in p.message for p in problems
                    ),
                    problems,
                )

    def test_repository_origin_checks_each_merge_parent_and_release_uniqueness(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            initial = self.initialize_repository_history(root)
            self.git(root, "switch", "-qc", "left")
            self.commit_state(
                root, "repository-root", "v0.1.0", "released", "left release"
            )
            self.git(root, "switch", "-qc", "right", initial)
            self.commit_state(
                root, "repository-root", "v0.1.0", "released", "right release"
            )
            self.git(root, "switch", "-q", "left")
            self.git(root, "merge", "--no-commit", "--no-ff", "right")
            self.write_release_artifacts(
                root, history_root="repository-root", release="v0.1.1", status="draft"
            )
            self.git(root, "add", ".")
            snapshot = self.gate_data("repository-root", "v0.1.1", "draft")
            for prospective in (True, False):
                if not prospective:
                    self.git(root, "commit", "-qm", "merge duplicate release")
                problems = self.validate_history(root, snapshot)
                self.assertTrue(
                    any("bound to multiple commits" in p.message for p in problems),
                    problems,
                )

    def test_repository_origin_enforces_normal_release_lifecycle(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.initialize_repository_history(root)
            self.commit_state(root, "repository-root", "v0.1.0", "released", "release")
            self.assertEqual(
                self.validate_history(
                    root, self.gate_data("repository-root", "v0.1.0", "released")
                ),
                [],
            )
            self.write_release_artifacts(
                root,
                history_root="repository-root",
                release="v0.1.1",
                status="released",
            )
            self.git(root, "add", ".")
            problems = self.validate_history(
                root, self.gate_data("repository-root", "v0.1.1", "released")
            )
            self.assertTrue(
                any("must start in draft" in p.message for p in problems), problems
            )

    def test_history_origin_literal_is_exact(self) -> None:
        self.assertEqual(
            release_identity.validate_history_root("repository-root", "root"),
            "repository-root",
        )
        for invalid in (
            "HEAD",
            "root",
            "Repository-root",
            "repository-root ",
            "",
            "a" * 39,
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                release_identity.validate_history_root(invalid, "root")


if __name__ == "__main__":
    unittest.main()

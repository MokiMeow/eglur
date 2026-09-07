"""Raw Git repository access for specification-release validation."""

import subprocess
from collections.abc import Sequence
from dataclasses import dataclass
from io import BytesIO
from pathlib import Path, PurePosixPath

GIT_HISTORY_COMMAND = ("git", "--no-replace-objects")


@dataclass(frozen=True)
class BlobQuery:
    """One historical commit and repository-relative artifact path."""

    commit: str
    path: PurePosixPath

    @property
    def object_spec(self) -> str:
        """Return the Git revision expression for this artifact."""
        return f"{self.commit}:{self.path.as_posix()}"


@dataclass(frozen=True)
class HistoryTopology:
    """Reachability heads and whether the index is a prospective state."""

    prospective: bool
    heads: tuple[str, ...]


@dataclass(frozen=True)
class HistoryRepositoryFeatures:
    """Git features that can make release history incomplete or virtual."""

    replacement_refs: tuple[str, ...]
    shallow: bool
    graft_metadata: bool


@dataclass(frozen=True)
class GitRepository:
    """Filesystem and subprocess boundary for one Git checkout."""

    root: Path

    def current_commit(self) -> str:
        """Return the full commit ID for the checked-out specification revision."""
        completed = subprocess.run(
            [*GIT_HISTORY_COMMAND, "rev-parse", "HEAD"],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or "git rev-parse HEAD failed"
            raise ValueError(f"cannot identify specification commit: {detail}")
        return completed.stdout.strip()

    def read_text_blobs(
        self,
        queries: Sequence[BlobQuery],
    ) -> dict[BlobQuery, str | None]:
        """Return historical UTF-8 blobs through one no-replace batch read."""
        unique_queries = list(dict.fromkeys(queries))
        if not unique_queries:
            return {}

        completed = subprocess.run(
            [*GIT_HISTORY_COMMAND, "cat-file", "--batch"],
            cwd=self.root,
            check=False,
            input=(
                "".join(f"{query.object_spec}\n" for query in unique_queries)
            ).encode(),
            capture_output=True,
        )
        if completed.returncode != 0:
            detail = completed.stderr.decode(errors="replace").strip()
            raise ValueError(
                "cannot batch-read release history artifacts: "
                f"{detail or 'git cat-file failed'}"
            )

        stream = BytesIO(completed.stdout)
        blobs: dict[BlobQuery, str | None] = {}
        for query in unique_queries:
            header = stream.readline()
            if not header:
                raise ValueError(
                    f"git cat-file omitted release history object {query.object_spec}"
                )
            if header.rstrip().endswith(b" missing"):
                blobs[query] = None
                continue

            fields = header.rstrip().split()
            if len(fields) != 3 or fields[1] != b"blob":
                raise ValueError(
                    f"git cat-file returned invalid header for {query.object_spec}"
                )
            try:
                size = int(fields[2])
            except ValueError as error:
                raise ValueError(
                    f"git cat-file returned invalid size for {query.object_spec}"
                ) from error
            content = stream.read(size)
            if len(content) != size or stream.read(1) != b"\n":
                raise ValueError(
                    f"git cat-file truncated release history object {query.object_spec}"
                )
            try:
                blobs[query] = content.decode("utf-8")
            except UnicodeDecodeError as error:
                raise ValueError(
                    f"release history artifact {query.object_spec} is not UTF-8"
                ) from error
        return blobs

    def staged_index_differs_from_head(self) -> bool:
        """Return whether the prospective index differs from HEAD."""
        staged = subprocess.run(
            [
                *GIT_HISTORY_COMMAND,
                "diff",
                "--cached",
                "--quiet",
                "HEAD",
                "--",
            ],
            cwd=self.root,
            check=False,
        )
        if staged.returncode == 1:
            return True
        if staged.returncode != 0:
            raise ValueError("cannot compare release state with specification HEAD")
        return False

    def merge_head_commits(self) -> tuple[str, ...]:
        """Return every in-progress merge parent recorded by Git."""
        completed = subprocess.run(
            [*GIT_HISTORY_COMMAND, "rev-parse", "--git-path", "MERGE_HEAD"],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or "git rev-parse failed"
            raise ValueError(f"cannot locate MERGE_HEAD: {detail}")
        merge_head_path = Path(completed.stdout.strip())
        if not merge_head_path.is_absolute():
            merge_head_path = self.root / merge_head_path
        if not merge_head_path.is_file():
            return ()
        return tuple(
            line.strip()
            for line in merge_head_path.read_text(encoding="ascii").splitlines()
            if line.strip()
        )

    def history_topology(self) -> HistoryTopology:
        """Return whether state is prospective and its reachable history heads."""
        head = self.current_commit()
        merge_heads = self.merge_head_commits()
        prospective = self.staged_index_differs_from_head() or bool(merge_heads)
        heads = [head]
        if prospective:
            heads.extend(merge_heads)
        return HistoryTopology(
            prospective=prospective,
            heads=tuple(dict.fromkeys(heads)),
        )

    def reachable_history(
        self,
        heads: tuple[str, ...],
        history_root: str | None = None,
    ) -> dict[str, tuple[str, ...]]:
        """Read original commit ancestry, optionally excluding a known past.

        Parameters
        ----------
        heads : tuple[str, ...]
            Commits whose combined ancestry is inspected.
        history_root : str or None
            Exclude this commit and its ancestors; None includes all ancestry.

        Returns
        -------
        dict[str, tuple[str, ...]]
            Commit IDs mapped to their complete ordered parent IDs.

        Raises
        ------
        ValueError
            If Git cannot enumerate the requested history.
        """
        completed = subprocess.run(
            [
                *GIT_HISTORY_COMMAND,
                "rev-list",
                "--parents",
                *heads,
                *(() if history_root is None else (f"^{history_root}",)),
            ],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or "git rev-list failed"
            raise ValueError(f"cannot enumerate release history: {detail}")

        history: dict[str, tuple[str, ...]] = {}
        for line in completed.stdout.splitlines():
            commit, *parents = line.split()
            history[commit] = tuple(parents)
        return history

    def commits_touching(
        self,
        history_root: str,
        path: PurePosixPath,
    ) -> tuple[str, ...]:
        """Return commits through a root that touched one release carrier."""
        completed = subprocess.run(
            [
                *GIT_HISTORY_COMMAND,
                "rev-list",
                history_root,
                "--",
                path.as_posix(),
            ],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or "git rev-list failed"
            raise ValueError(f"cannot inspect release history root: {detail}")
        return tuple(completed.stdout.splitlines())

    def history_repository_features(self) -> HistoryRepositoryFeatures:
        """Return repository facts that affect release-history integrity."""
        replacements = subprocess.run(
            [
                *GIT_HISTORY_COMMAND,
                "for-each-ref",
                "--format=%(refname)",
                "refs/replace/",
            ],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
        )
        if replacements.returncode != 0:
            detail = replacements.stderr.strip() or "git for-each-ref failed"
            raise ValueError(f"cannot inspect Git replacement refs: {detail}")

        shallow = subprocess.run(
            [*GIT_HISTORY_COMMAND, "rev-parse", "--is-shallow-repository"],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
        )
        if shallow.returncode != 0:
            detail = shallow.stderr.strip() or "git rev-parse failed"
            raise ValueError(f"cannot inspect shallow repository state: {detail}")

        graft_path_result = subprocess.run(
            [*GIT_HISTORY_COMMAND, "rev-parse", "--git-path", "info/grafts"],
            cwd=self.root,
            check=False,
            capture_output=True,
            text=True,
        )
        if graft_path_result.returncode != 0:
            detail = graft_path_result.stderr.strip() or "git rev-parse failed"
            raise ValueError(f"cannot locate Git graft metadata: {detail}")
        graft_path = Path(graft_path_result.stdout.strip())
        if not graft_path.is_absolute():
            graft_path = self.root / graft_path

        return HistoryRepositoryFeatures(
            replacement_refs=tuple(replacements.stdout.splitlines()),
            shallow=shallow.stdout.strip() == "true",
            graft_metadata=(
                graft_path.is_file()
                and bool(graft_path.read_text(encoding="ascii").strip())
            ),
        )

    def tracked_checkout_differs_from_head(self) -> bool:
        """Return whether tracked worktree or index content differs from HEAD."""
        commands = (
            [*GIT_HISTORY_COMMAND, "diff", "--quiet", "HEAD", "--"],
            [
                *GIT_HISTORY_COMMAND,
                "diff",
                "--cached",
                "--quiet",
                "HEAD",
                "--",
            ],
        )
        for command in commands:
            completed = subprocess.run(command, cwd=self.root, check=False)
            if completed.returncode == 1:
                return True
            if completed.returncode != 0:
                raise ValueError(
                    f"cannot inspect specification checkout: {' '.join(command)}"
                )
        return False

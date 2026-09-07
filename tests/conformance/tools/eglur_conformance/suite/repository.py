"""Load the canonical conformance bundle from a repository checkout."""

from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from types import MappingProxyType

from eglur_conformance.suite import artifact, model, schema
from eglur_conformance.yaml_document import load_yaml_text


@dataclass(frozen=True)
class ConformanceInputs:
    """Canonical suite model paired with its exact serialized inputs.

    Parameters
    ----------
    bundle : model.ConformanceBundle
        Strictly decoded manifest and suite documents.
    manifest_bytes : bytes
        Exact manifest bytes.
    suite_document_bytes : Mapping[str, bytes]
        Exact suite-document bytes keyed by manifest-relative path.
    """

    bundle: model.ConformanceBundle
    manifest_bytes: bytes
    suite_document_bytes: Mapping[str, bytes]

    @property
    def content_digest(self) -> str:
        """Return the address of the complete serialized bundle.

        Returns
        -------
        str
            Canonical SHA-256 address of the manifest and suite documents.
        """
        return artifact.conformance_bundle_digest(
            self.manifest_bytes,
            self.suite_document_bytes,
        )


def load_conformance_inputs(repo_root: Path) -> ConformanceInputs:
    """Load one checkout's exact manifest-owned conformance bundle.

    Parameters
    ----------
    repo_root : pathlib.Path
        Repository root containing the canonical conformance manifest.

    Returns
    -------
    ConformanceInputs
        Decoded bundle and immutable snapshots of every serialized input.

    Raises
    ------
    TypeError
        ``repo_root`` is not a ``Path``.
    ValueError
        ``repo_root`` is not a directory or suite data violates its contract.
    OSError
        A required artifact cannot be read.
    UnicodeDecodeError
        A required YAML artifact is not valid UTF-8.
    yaml.YAMLError
        A required artifact is not valid YAML.
    """
    if not isinstance(repo_root, Path):
        raise TypeError("repository root must be a Path")
    root = repo_root.resolve(strict=True)
    if not root.is_dir():
        raise ValueError(f"repository root is not a directory: {repo_root}")

    manifest_path = root / schema.DEFAULT_MANIFEST_PATH
    manifest_bytes = manifest_path.read_bytes()
    manifest = schema.parse_manifest_value(
        load_yaml_text(manifest_bytes.decode("utf-8")),
        str(manifest_path),
    )
    suite_document_bytes = {
        entry.file: (root / manifest.root / entry.file).read_bytes()
        for entry in manifest.suites
    }
    snapshot = artifact.snapshot_suite_document_bytes(suite_document_bytes)
    return ConformanceInputs(
        bundle=artifact.parse_conformance_bundle_bytes(manifest_bytes, snapshot),
        manifest_bytes=memoryview(manifest_bytes).tobytes(),
        suite_document_bytes=MappingProxyType(snapshot),
    )

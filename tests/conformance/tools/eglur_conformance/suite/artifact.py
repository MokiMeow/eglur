"""Own exact serialized conformance-suite artifacts."""

from collections.abc import Mapping
from typing import cast

from eglur_conformance.artifact_content import (
    JsonValue,
    canonical_digest,
    expect_text,
    raw_artifact_digest,
    snapshot_bytes,
)
from eglur_conformance.suite import codec, model
from eglur_conformance.yaml_document import YamlValue, load_yaml_text


def conformance_bundle_digest(
    manifest_bytes: bytes,
    suite_document_bytes: Mapping[str, bytes],
) -> str:
    """Address an exact manifest and suite-document byte set.

    Parameters
    ----------
    manifest_bytes : bytes
        Exact serialized manifest content.
    suite_document_bytes : Mapping[str, bytes]
        Exact serialized suite documents keyed by manifest-relative path.

    Returns
    -------
    str
        Canonical SHA-256 address of the complete conformance bundle.

    Raises
    ------
    TypeError
        If the manifest, mapping, path, or suite content has the wrong type.
    ValueError
        If a suite path is empty or repeated after validation.
    """
    manifest_snapshot = snapshot_bytes(
        manifest_bytes,
        "conformance_bundle.manifest_bytes",
    )
    suite_document_snapshot = snapshot_suite_document_bytes(suite_document_bytes)
    suite_document_digests: dict[str, JsonValue] = {
        path: raw_artifact_digest(content)
        for path, content in suite_document_snapshot.items()
    }
    return canonical_digest(
        {
            "manifest_digest": raw_artifact_digest(manifest_snapshot),
            "suite_document_digests": suite_document_digests,
        }
    )


def snapshot_suite_document_bytes(
    suite_document_bytes: Mapping[str, bytes],
) -> dict[str, bytes]:
    """Snapshot suite-document paths and immutable byte values.

    Parameters
    ----------
    suite_document_bytes : Mapping[str, bytes]
        Serialized suite documents keyed by manifest-relative path.

    Returns
    -------
    dict[str, bytes]
        One stable snapshot preserving every path and exact byte sequence.

    Raises
    ------
    TypeError
        If the input is not a mapping or a path or value has the wrong type.
    ValueError
        If a path is empty or repeated after validation.
    """
    if not isinstance(suite_document_bytes, Mapping):
        raise TypeError("conformance_bundle.suite_document_bytes must be a mapping")
    snapshot: dict[str, bytes] = {}
    for raw_path, content in suite_document_bytes.items():
        path = expect_text(
            cast(YamlValue, raw_path),
            "conformance_bundle.suite_document_bytes key",
            allow_empty=False,
        )
        if path in snapshot:
            raise ValueError(
                "conformance_bundle.suite_document_bytes must not repeat paths"
            )
        snapshot[path] = snapshot_bytes(
            content,
            f"conformance_bundle.suite_document_bytes[{path!r}]",
        )
    return snapshot


def parse_conformance_bundle_bytes(
    manifest_bytes: bytes,
    suite_document_bytes: Mapping[str, bytes],
) -> model.ConformanceBundle:
    """Decode an exact UTF-8 conformance bundle.

    Parameters
    ----------
    manifest_bytes : bytes
        Exact serialized manifest content.
    suite_document_bytes : Mapping[str, bytes]
        Exact serialized suite documents keyed by manifest-relative path.

    Returns
    -------
    ConformanceBundle
        Canonical suite model decoded from the serialized artifact set.

    Raises
    ------
    TypeError
        If an input or decoded document has the wrong type.
    ValueError
        If a suite path or decoded document violates the suite contract.
    UnicodeDecodeError
        If the manifest or a suite document is not valid UTF-8.
    yaml.YAMLError
        If the manifest or a suite document is not valid YAML.
    """
    manifest_snapshot = snapshot_bytes(
        manifest_bytes,
        "conformance_bundle.manifest_bytes",
    )
    suite_document_snapshot = snapshot_suite_document_bytes(suite_document_bytes)
    manifest = load_yaml_text(manifest_snapshot.decode("utf-8"))
    suite_documents = {
        expect_text(
            cast(YamlValue, raw_path),
            "conformance_bundle.suite_document_bytes key",
            allow_empty=False,
        ): load_yaml_text(content.decode("utf-8"))
        for raw_path, content in suite_document_snapshot.items()
    }
    return codec.decode_bundle(manifest, suite_documents)

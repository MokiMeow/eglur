"""Load and validate the repository registry-cache probe definition."""

from pathlib import Path

from eglur_conformance.registry_cache import codec, policy
from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import load_yaml

DEFAULT_PROFILE_PATH = Path("tests/conformance/probes/registry_cache.yaml")


def validate_repository(
    repo_root: Path,
    manifest_path: Path,
    profile_path: Path = DEFAULT_PROFILE_PATH,
) -> list[ValidationProblem]:
    """Validate the cache probe and all source-fixture references.

    Parameters
    ----------
    repo_root : Path
        Repository root containing the conformance package.
    manifest_path : Path
        Absolute or repository-relative path to the source-fixture manifest.
    profile_path : Path
        Absolute or repository-relative path to the cache probe artifact.

    Returns
    -------
    list[ValidationProblem]
        Semantic profile failures. Malformed documents raise decoding errors.
    """
    resolved_profile_path = (
        profile_path if profile_path.is_absolute() else repo_root / profile_path
    )
    profile = codec.decode_profile(
        load_yaml(resolved_profile_path),
        str(resolved_profile_path),
    )
    resolved_manifest_path = (
        manifest_path if manifest_path.is_absolute() else repo_root / manifest_path
    ).resolve()
    declared_manifest_path = (repo_root / profile.fixture_manifest).resolve()
    if resolved_manifest_path != declared_manifest_path:
        return [
            ValidationProblem(
                "registry-cache profile fixture_manifest does not match the selected "
                f"manifest: {profile.fixture_manifest}"
            )
        ]
    manifest_value = load_yaml(resolved_manifest_path)
    manifest = suite_schema.parse_manifest_value(
        manifest_value,
        str(resolved_manifest_path),
    )
    suite_documents = {
        entry.file: load_yaml(repo_root / manifest.root / entry.file)
        for entry in manifest.suites
    }
    bundle = suite_codec.decode_bundle(manifest_value, suite_documents)
    return policy.validate_profile(profile, bundle)

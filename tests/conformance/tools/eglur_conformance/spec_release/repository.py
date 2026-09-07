"""Load one canonical specification-package snapshot from a repository."""

from dataclasses import dataclass
from pathlib import Path
from types import MappingProxyType

from eglur_conformance.spec_release import artifact_inventory, open_items, release_state
from eglur_conformance.suite import artifact as suite_artifact
from eglur_conformance.suite import model as suite_model
from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.traceability import model as traceability_model
from eglur_conformance.traceability import repository as traceability_repository
from eglur_conformance.yaml_document import load_yaml

DEFAULT_MANIFEST_PATH = Path("tests/conformance/manifest.yaml")
DEFAULT_TRACEABILITY_PATH = traceability_repository.TRACEABILITY_PATH
DEFAULT_LEDGER_PATH = open_items.DEFAULT_LEDGER_PATH
DEFAULT_INVENTORY_PATH = artifact_inventory.DEFAULT_INVENTORY_PATH
RELEASE_STATE_PATHS = MappingProxyType(
    {
        "artifact_inventory": DEFAULT_INVENTORY_PATH,
        "manifest": DEFAULT_MANIFEST_PATH,
        "traceability": DEFAULT_TRACEABILITY_PATH,
        "open_items_ledger": DEFAULT_LEDGER_PATH,
    }
)


@dataclass(frozen=True)
class SpecPackageSnapshot:
    """Canonical release-gate inputs loaded from one repository state."""

    inventory: artifact_inventory.ArtifactInventory
    manifest: suite_model.Manifest
    conformance_bundle_digest: str
    traceability: traceability_model.TraceabilityReport
    ledger: open_items.Ledger

    def release_carriers(self) -> tuple[release_state.ReleaseCarrier, ...]:
        """Project artifact-owned release state into one canonical carrier set."""
        return (
            release_state.ReleaseCarrier(
                name="artifact_inventory",
                history_root=self.inventory.release_history_root,
                release=self.inventory.release,
                status=self.inventory.status,
            ),
            release_state.ReleaseCarrier(
                name="manifest",
                history_root=self.manifest.release_history_root,
                release=self.manifest.release,
                status=self.manifest.status,
            ),
            release_state.ReleaseCarrier(
                name="traceability",
                history_root=self.traceability.release_history_root,
                release=self.traceability.release.text,
                status=self.traceability.status,
            ),
            release_state.ReleaseCarrier(
                name="open_items_ledger",
                history_root=self.ledger.release_history_root,
                release=self.ledger.release,
                status=self.ledger.status,
            ),
        )


def load_snapshot(repo_root: Path) -> SpecPackageSnapshot:
    """Load inventory, manifest, traceability, and OPEN-ledger gate data."""
    inventory_path = repo_root / DEFAULT_INVENTORY_PATH
    inventory = artifact_inventory.decode_inventory(
        load_yaml(inventory_path),
        str(inventory_path),
    )
    manifest_path = repo_root / DEFAULT_MANIFEST_PATH
    manifest = suite_schema.parse_manifest(manifest_path)
    bundle_digest = suite_artifact.conformance_bundle_digest(
        manifest_path.read_bytes(),
        {
            suite.file: (repo_root / manifest.root / suite.file).read_bytes()
            for suite in manifest.suites
        },
    )
    traceability = traceability_repository.read_report(repo_root)
    ledger_path = repo_root / DEFAULT_LEDGER_PATH
    ledger = open_items.decode_ledger(load_yaml(ledger_path), str(ledger_path))
    return SpecPackageSnapshot(
        inventory=inventory,
        manifest=manifest,
        conformance_bundle_digest=bundle_digest,
        traceability=traceability,
        ledger=ledger,
    )

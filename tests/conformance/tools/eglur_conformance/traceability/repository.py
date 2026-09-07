"""Repository boundary for diagnostic traceability inputs and artifacts."""

from pathlib import Path

from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics.registry import load_diagnostic_registry
from eglur_conformance.markdown_document import visible_markdown_lines
from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import schema as conformance_schema
from eglur_conformance.traceability import analysis, codec, model
from eglur_conformance.yaml_document import load_yaml

TRACEABILITY_PATH = Path("tests/conformance/traceability.yaml")
MANIFEST_PATH = Path("tests/conformance/manifest.yaml")
DIAGNOSTIC_CATALOG_PATHS = (
    Path("docs/spec/errors/compile-codes.md"),
    Path("docs/spec/errors/extension-codes.md"),
    Path("docs/spec/errors/runtime-codes.md"),
)
DIAGNOSTIC_ERRORS_PATH = Path("docs/spec/errors")


def build_report(repo_root: Path) -> model.TraceabilityReport:
    """Build one report from authoritative repository sources."""
    contract_path = diagnostic_contracts.DEFAULT_CONTRACT_PATH
    contracts = diagnostic_contracts.load_contracts(repo_root / contract_path)
    registry = load_diagnostic_registry(
        repo_root / DIAGNOSTIC_ERRORS_PATH,
        contracts,
    )

    metadata: dict[str, analysis.CodeMetadata] = {}
    for relative_path in DIAGNOSTIC_CATALOG_PATHS:
        source_path = repo_root / relative_path
        metadata.update(
            analysis.extract_code_metadata(
                str(relative_path),
                visible_markdown_lines(source_path),
            )
        )

    manifest_path = repo_root / MANIFEST_PATH
    manifest_value = load_yaml(manifest_path)
    manifest = conformance_schema.parse_manifest_value(
        manifest_value,
        str(manifest_path),
    )
    suite_documents: dict[str, diagnostic_contracts.YamlValue] = {}
    for entry in manifest.suites:
        suite_path = repo_root / manifest.root / entry.file
        if not suite_path.is_file():
            raise ValueError(
                f"manifest suite file is missing: tests/conformance/{entry.file}"
            )
        suite_documents[entry.file] = load_yaml(suite_path)
    bundle = suite_codec.decode_bundle(
        manifest_value,
        suite_documents,
        context="traceability",
    )

    return analysis.build_traceability_report(
        bundle=bundle,
        code_metadata=metadata,
        contracts=contracts,
        channel_registry=registry,
        sources=model.TraceabilitySources(
            diagnostic_catalogs=tuple(map(str, DIAGNOSTIC_CATALOG_PATHS)),
            diagnostic_contract=str(contract_path),
            suites_manifest=str(MANIFEST_PATH),
        ),
        optional_probe_policy=model.OptionalProbePolicy(
            warning_threshold_min_asserted=(
                model.OPTIONAL_PROBE_WARNING_THRESHOLD_MIN_ASSERTED
            )
        ),
        contract_file=str(contract_path),
    )


def read_report(repo_root: Path) -> model.TraceabilityReport:
    """Decode the checked-in traceability artifact."""
    path = repo_root / TRACEABILITY_PATH
    return codec.decode_report(load_yaml(path))

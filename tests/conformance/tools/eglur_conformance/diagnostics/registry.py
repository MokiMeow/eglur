"""Compose diagnostic channel and coverage profiles by code."""

from pathlib import Path

from eglur_conformance.diagnostics import catalog, contracts, coverage
from eglur_conformance.diagnostics.model import DiagnosticProfile, DiagnosticRegistry


def load_diagnostic_registry(
    errors_dir: Path,
    diagnostic_contracts: dict[str, contracts.DiagnosticContract],
) -> DiagnosticRegistry:
    """Load one registry without conflating channel and coverage ownership."""
    diagnostic_catalog = catalog.load_catalog(errors_dir)
    channel_profiles = catalog.derive_channel_profiles(
        diagnostic_catalog,
        diagnostic_contracts,
    )
    coverage_obligations = coverage.derive_coverage_obligations(diagnostic_contracts)
    if set(channel_profiles) != set(coverage_obligations):
        raise ValueError("diagnostic channel and coverage profile sets do not match")
    return {
        code: DiagnosticProfile(
            channels=channel_profiles[code],
            coverage=coverage_obligations[code],
        )
        for code in diagnostic_contracts
    }

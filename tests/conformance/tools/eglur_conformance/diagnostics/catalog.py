"""Load diagnostic catalog membership and derive output channels."""

import re
from dataclasses import dataclass
from pathlib import Path

from eglur_conformance.diagnostics import contracts
from eglur_conformance.diagnostics.model import (
    COMPILE_ERROR,
    RUNTIME_ERROR,
    VALUE_DIAGNOSTICS,
    DiagnosticChannel,
    DiagnosticChannelProfile,
)
from eglur_conformance.markdown_document import visible_markdown_lines

RE_CODE_HEADER = re.compile(r"^###\s+`([A-Z][A-Z0-9_]+)`\s*$")


@dataclass(frozen=True)
class DiagnosticCatalog:
    """Disjoint code sets from the three normative diagnostic catalogs."""

    compile_codes: frozenset[str]
    extension_codes: frozenset[str]
    runtime_codes: frozenset[str]

    @property
    def codes(self) -> frozenset[str]:
        """Return every catalogued diagnostic code."""
        return self.compile_codes | self.extension_codes | self.runtime_codes


def extract_codes(path: Path) -> frozenset[str]:
    """Return diagnostic codes declared by one Markdown catalog."""
    codes: set[str] = set()
    for line_number, line in visible_markdown_lines(path):
        matched = RE_CODE_HEADER.fullmatch(line.strip())
        if matched is None:
            continue
        code = matched.group(1)
        if code in codes:
            raise ValueError(f"{path}:{line_number}: duplicate diagnostic code {code}")
        codes.add(code)
    return frozenset(codes)


def load_catalog(errors_dir: Path) -> DiagnosticCatalog:
    """Load three disjoint diagnostic catalogs."""
    catalog = DiagnosticCatalog(
        compile_codes=extract_codes(errors_dir / "compile-codes.md"),
        extension_codes=extract_codes(errors_dir / "extension-codes.md"),
        runtime_codes=extract_codes(errors_dir / "runtime-codes.md"),
    )
    cross_catalog_duplicates = sorted(
        (catalog.compile_codes & catalog.extension_codes)
        | (catalog.compile_codes & catalog.runtime_codes)
        | (catalog.extension_codes & catalog.runtime_codes)
    )
    if cross_catalog_duplicates:
        raise ValueError(
            f"diagnostic codes appear in multiple catalogs: {cross_catalog_duplicates}"
        )
    return catalog


def derive_channel_profiles(
    catalog: DiagnosticCatalog,
    diagnostic_contracts: dict[str, contracts.DiagnosticContract],
) -> dict[str, DiagnosticChannelProfile]:
    """Derive allowed output channels from catalog membership and severity."""
    missing_contracts = sorted(catalog.codes - set(diagnostic_contracts))
    unknown_contracts = sorted(set(diagnostic_contracts) - catalog.codes)
    if missing_contracts or unknown_contracts:
        raise ValueError(
            "diagnostic catalog/contract code mismatch: "
            f"missing contracts={missing_contracts}, "
            f"unknown contracts={unknown_contracts}"
        )

    profiles: dict[str, DiagnosticChannelProfile] = {}
    for code, contract in diagnostic_contracts.items():
        allowed: set[DiagnosticChannel] = set()
        if code in catalog.compile_codes or code in catalog.extension_codes:
            allowed.add(COMPILE_ERROR)

        if code in catalog.runtime_codes:
            if code == "DETERMINISM_UNAVAILABLE":
                allowed = {VALUE_DIAGNOSTICS}
            elif code == "RUNTIME_ABORT":
                allowed = {RUNTIME_ERROR}
            elif code == "EXTERN_LINKAGE_FAILED":
                allowed.update({COMPILE_ERROR, RUNTIME_ERROR})
            elif contract.severity == "error":
                allowed.update({COMPILE_ERROR, VALUE_DIAGNOSTICS})
            else:
                allowed.add(VALUE_DIAGNOSTICS)

        if not allowed:
            raise ValueError(f"diagnostic code {code} has no allowed channel")
        profiles[code] = DiagnosticChannelProfile(allowed_channels=frozenset(allowed))
    return profiles

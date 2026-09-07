"""Derive baseline conformance obligations for diagnostic codes."""

from eglur_conformance.diagnostics import contracts
from eglur_conformance.diagnostics.model import (
    CONDITIONAL_REQUIRED,
    EXCLUDED_NONBASELINE,
    HARD_REQUIRED_ASSERTED,
    OPTIONAL_EMIT_IF_PRESENT,
    DiagnosticCoverageObligation,
)

EXCLUDED_NONBASELINE_CODES = frozenset(
    {
        "AXIS_ALIASED_HEADER",
        "DERIVE_EXPANSION_FORBIDDEN",
        "GRAD_CUSTOM_KERNEL_NO_VJP",
        "GRAD_RESULT_SHAPE_MISMATCH",
        "OWNERSHIP_PROFILE_DISABLED",
    }
)
CONDITIONAL_REQUIRED_CODES = {
    "DETERMINISM_UNAVAILABLE": "strict_unavailable_path",
    "EXTERN_LINKAGE_FAILED": "ffi_linkage_probe",
}


def derive_coverage_obligations(
    diagnostic_contracts: dict[str, contracts.DiagnosticContract],
) -> dict[str, DiagnosticCoverageObligation]:
    """Derive coverage obligations independently from output-channel routing."""
    policy_codes = EXCLUDED_NONBASELINE_CODES | set(CONDITIONAL_REQUIRED_CODES)
    unknown_policy_codes = sorted(policy_codes - set(diagnostic_contracts))
    if unknown_policy_codes:
        raise ValueError(
            "diagnostic coverage policy references unknown codes: "
            f"{unknown_policy_codes}"
        )

    obligations: dict[str, DiagnosticCoverageObligation] = {}
    for code, contract in diagnostic_contracts.items():
        if code in EXCLUDED_NONBASELINE_CODES:
            obligation = DiagnosticCoverageObligation(
                policy=EXCLUDED_NONBASELINE,
                precondition=None,
            )
        elif code in CONDITIONAL_REQUIRED_CODES:
            obligation = DiagnosticCoverageObligation(
                policy=CONDITIONAL_REQUIRED,
                precondition=CONDITIONAL_REQUIRED_CODES[code],
            )
        elif contract.severity in {"note", "warning"}:
            obligation = DiagnosticCoverageObligation(
                policy=OPTIONAL_EMIT_IF_PRESENT,
                precondition=None,
            )
        else:
            obligation = DiagnosticCoverageObligation(
                policy=HARD_REQUIRED_ASSERTED,
                precondition=None,
            )
        obligations[code] = obligation
    return obligations

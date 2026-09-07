"""Closed conformance precondition registry."""

from typing import Final

V01_PRECONDITIONS: Final[frozenset[str]] = frozenset(
    {
        "console_write_failure_probe",
        "ffi_linkage_probe",
        "par_traverse_quiescence_probe",
        "scoped_close_failure_probe",
        "strict_unavailable_path",
    }
)

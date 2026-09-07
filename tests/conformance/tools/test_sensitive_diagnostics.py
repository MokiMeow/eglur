"""Protect representative diagnostic assertions from silent weakening."""

import unittest
from dataclasses import dataclass
from pathlib import Path
from typing import ClassVar

from eglur_conformance.diagnostics.model import DiagnosticScalar
from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import model as suite_model
from eglur_conformance.suite import schema as suite_schema
from eglur_conformance.yaml_document import load_yaml

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


@dataclass(frozen=True)
class SensitiveDiagnosticExpectation:
    """Required assertion surface for one representative compile diagnostic."""

    code: str
    marker: str
    data_keys: tuple[str, ...]
    data_equals: tuple[tuple[str, DiagnosticScalar], ...] = ()


SENSITIVE_DIAGNOSTICS: dict[str, SensitiveDiagnosticExpectation] = {
    "NUM-const-overflow-retains-int-overflow-code": SensitiveDiagnosticExpectation(
        "INT_OVERFLOW",
        "const-int-overflow",
        ("operation", "dtype"),
    ),
    "NUM-const-division-by-zero-retains-int-div-by-zero-code": (
        SensitiveDiagnosticExpectation(
            "INT_DIV_BY_ZERO",
            "const-int-division-by-zero",
            ("operation", "dtype"),
        )
    ),
    "NUM-const-remainder-by-zero-retains-int-rem-by-zero-code": (
        SensitiveDiagnosticExpectation(
            "INT_REM_BY_ZERO",
            "const-int-remainder-by-zero",
            ("operation", "dtype"),
        )
    ),
    "CST-forbidden-dtype-pair-fails-with-cast-invalid-dtype-pair-in-const-evaluable-context": (
        SensitiveDiagnosticExpectation(
            "CAST_INVALID_DTYPE_PAIR",
            "forbidden-cast-dtype-pair",
            ("src_dtype", "dst_dtype"),
        )
    ),
    "CST-const-evaluable-nan-to-int-cast-fails-with-cast-invalid-value": (
        SensitiveDiagnosticExpectation(
            "CAST_INVALID_VALUE",
            "invalid-const-cast-value",
            ("src_dtype", "dst_dtype"),
        )
    ),
    "XFM-non-distributive-equal-numel-axisexpr-map-is-unsupported": (
        SensitiveDiagnosticExpectation(
            "AXIS_MAPPING_UNSUPPORTED",
            "unsupported-axis-mapping",
            (),
        )
    ),
    "XFM-output-only-free-dim-is-ambiguous": SensitiveDiagnosticExpectation(
        "AMBIGUOUS_DIMS",
        "ambiguous-output-dimension",
        ("operation",),
    ),
    "XFM-with-sizes-conflicting-binding-is-inconsistent-dims": (
        SensitiveDiagnosticExpectation(
            "INCONSISTENT_DIMS",
            "conflicting-size-binding",
            ("operation",),
        )
    ),
    "XFM-rearrange-rejects-output-segments-that-straddle-operands": (
        SensitiveDiagnosticExpectation(
            "SEGMENT_STRADDLE",
            "straddling-output-segment",
            (),
        )
    ),
    "RDX-const-empty-max-domain-fails-before-empty-output-short-circuit": (
        SensitiveDiagnosticExpectation(
            "REDUCTION_EMPTY_DOMAIN",
            "empty-max-domain",
            ("operation", "axis"),
            (("axis", 0), ("operation", "max")),
        )
    ),
    "TIX-get-at-const-out-of-bounds-is-compile-time-index-out-of-bounds": (
        SensitiveDiagnosticExpectation(
            "INDEX_OUT_OF_BOUNDS",
            "const-index-out-of-bounds",
            (),
        )
    ),
    "TIX-set-at-assign-with-normalized-aliases-is-compile-time-scatter-set-duplicate-index": (
        SensitiveDiagnosticExpectation(
            "SCATTER_SET_DUPLICATE_INDEX",
            "duplicate-scatter-index",
            ("index",),
        )
    ),
    "TIX-const-duplicate-rest-is-rejected": SensitiveDiagnosticExpectation(
        "INDEX_SPEC_REST_DUPLICATE",
        "duplicate-index-rest",
        ("count",),
        (("count", 2),),
    ),
    "TIX-const-over-consuming-index-spec-is-rejected": (
        SensitiveDiagnosticExpectation(
            "INDEX_SPEC_ARITY_MISMATCH",
            "index-spec-arity-mismatch",
            ("source_rank", "consuming_items"),
            (("consuming_items", 3), ("source_rank", 2)),
        )
    ),
}


class SensitiveDiagnosticTests(unittest.TestCase):
    """Keep a bounded set of high-risk compile diagnostics fully asserted."""

    fixtures: ClassVar[dict[str, suite_model.TestCase]]

    @classmethod
    def setUpClass(cls) -> None:
        """Decode required suites and index their fixtures once."""
        manifest_path = REPO_ROOT / suite_schema.DEFAULT_MANIFEST_PATH
        manifest_value = load_yaml(manifest_path)
        manifest = suite_schema.parse_manifest_value(
            manifest_value,
            str(manifest_path),
        )
        suite_documents = {
            entry.file: load_yaml(REPO_ROOT / manifest.root / entry.file)
            for entry in manifest.suites
        }
        bundle = suite_codec.decode_bundle(manifest_value, suite_documents)

        cls.fixtures = {
            test.test_id: test
            for suite in bundle.suites
            if suite.entry.required
            for test in suite.document.tests
        }

    def test_sensitive_diagnostics_retain_location_and_payload(self) -> None:
        """Reject outcome, location, and portable-payload assertion drift."""
        for fixture_id, expected in SENSITIVE_DIAGNOSTICS.items():
            with self.subTest(fixture_id=fixture_id):
                fixture = self.fixtures.get(fixture_id)
                self.assertIsNotNone(fixture, "fixture must remain required")
                if fixture is None:
                    continue

                outcome = fixture.expectation.outcome
                self.assertIsInstance(outcome, suite_model.CompileErrorOutcome)
                if not isinstance(outcome, suite_model.CompileErrorOutcome):
                    continue

                diagnostic = outcome.diagnostic
                self.assertEqual(diagnostic.code, expected.code)
                self.assertEqual(diagnostic.data_keys, expected.data_keys)
                self.assertEqual(diagnostic.data_equals, expected.data_equals)
                self.assertIsNotNone(diagnostic.primary_span)
                if diagnostic.primary_span is None:
                    continue
                self.assertIsInstance(
                    diagnostic.primary_span,
                    suite_model.MarkerPrimarySpan,
                )
                if not isinstance(
                    diagnostic.primary_span,
                    suite_model.MarkerPrimarySpan,
                ):
                    continue
                self.assertEqual(diagnostic.primary_span.marker, expected.marker)
                self.assertEqual(diagnostic.primary_span.source, "code")


if __name__ == "__main__":
    unittest.main()

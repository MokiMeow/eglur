"""Mutation tests for canonical traceability projections and exact encoding."""

import unittest
from copy import deepcopy
from pathlib import Path
from typing import cast

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


from eglur_conformance.traceability import codec, repository
from eglur_conformance.yaml_document import YamlValue, load_yaml


class TraceabilityCodecTests(unittest.TestCase):
    """Require all persisted report projections to agree with canonical facts."""

    def document(self) -> dict[str, YamlValue]:
        """Return a mutable copy of the checked-in traceability document."""
        value = load_yaml(REPO_ROOT / repository.TRACEABILITY_PATH)
        self.assertIsInstance(value, dict)
        return cast(dict[str, YamlValue], deepcopy(value))

    def mapping(
        self,
        value: YamlValue,
        context: str,
    ) -> dict[str, YamlValue]:
        """Narrow a fixture mapping for mutation."""
        self.assertIsInstance(value, dict, context)
        return cast(dict[str, YamlValue], value)

    def sequence(self, value: YamlValue, context: str) -> list[YamlValue]:
        """Narrow a fixture sequence for mutation."""
        self.assertIsInstance(value, list, context)
        return value

    def test_checked_in_report_round_trips_byte_exactly(self) -> None:
        report = codec.decode_report(self.document())

        self.assertEqual(
            codec.encode_report(report),
            (REPO_ROOT / repository.TRACEABILITY_PATH).read_bytes(),
        )
        self.assertEqual(report.summary.diagnostic_code_count, 272)

    def test_rejects_every_mutated_summary_projection(self) -> None:
        canonical = self.document()
        summary = self.mapping(canonical["summary"], "summary")

        for key, value in summary.items():
            with self.subTest(key=key):
                self.assertIsInstance(value, int)
                mutation = deepcopy(canonical)
                mutated_summary = self.mapping(mutation["summary"], "summary")
                mutated_summary[key] = cast(int, value) + 1

                with self.assertRaisesRegex(
                    ValueError,
                    rf"traceability\.summary\.{key}",
                ):
                    codec.decode_report(mutation)

    def test_rejects_mutated_duplicate_projections(self) -> None:
        mutators = {
            "policy.excluded_nonbaseline_codes": self._drop_policy_excluded,
            "policy.conditional_required_codes": self._drop_policy_conditional,
            "optional_probe_summary.optional_suites": self._drop_probe_suite,
            "optional_extension_summary.optional_suites": (self._drop_extension_suite),
            "uncovered_codes": self._drop_uncovered,
            "hard_gaps": self._invent_hard_gap,
            "conditional_pending": self._invent_conditional_pending,
            "optional_emit_unasserted": self._drop_optional_unasserted,
            "excluded_nonbaseline_codes": self._drop_excluded,
        }
        for projection, mutate in mutators.items():
            with self.subTest(projection=projection):
                mutation = self.document()
                mutate(mutation)
                with self.assertRaisesRegex(ValueError, projection):
                    codec.decode_report(mutation)

    def test_rejects_derived_warning_and_failure_count_mutations(self) -> None:
        warning_active = self.document()
        probe_summary = self.mapping(
            warning_active["optional_probe_summary"],
            "optional_probe_summary",
        )
        probe_summary["warning_active"] = True
        with self.assertRaisesRegex(ValueError, "warning_active"):
            codec.decode_report(warning_active)

        warning_message = self.document()
        probe_summary = self.mapping(
            warning_message["optional_probe_summary"],
            "optional_probe_summary",
        )
        probe_summary["warning_message"] = "not derived from evidence"
        with self.assertRaisesRegex(ValueError, "warning_message"):
            codec.decode_report(warning_message)

        failure_count = self.document()
        contract_validation = self.mapping(
            failure_count["contract_validation"],
            "contract_validation",
        )
        contract_validation["failure_count"] = 1
        with self.assertRaisesRegex(ValueError, "failure_count"):
            codec.decode_report(failure_count)

    def test_rejects_statuses_and_optional_code_partitions_not_derived_from_evidence(
        self,
    ) -> None:
        required_status = self.document()
        coverage = self.sequence(required_status["coverage"], "coverage")
        first_trace = self.mapping(coverage[0], "coverage[0]")
        first_trace["status"] = "uncovered"
        with self.assertRaisesRegex(ValueError, r"coverage\[0\]\.status"):
            codec.decode_report(required_status)

        optional_status = self.document()
        optional_coverage = self.sequence(
            optional_status["optional_probe_coverage"],
            "optional_probe_coverage",
        )
        first_optional = self.mapping(
            optional_coverage[0],
            "optional_probe_coverage[0]",
        )
        first_optional["status"] = "unasserted"
        with self.assertRaisesRegex(
            ValueError,
            r"optional_probe_coverage\[0\]\.status",
        ):
            codec.decode_report(optional_status)

        missing_optional_code = self.document()
        optional_coverage = self.sequence(
            missing_optional_code["optional_probe_coverage"],
            "optional_probe_coverage",
        )
        optional_coverage.pop()
        with self.assertRaisesRegex(ValueError, "optional_probe_coverage codes"):
            codec.decode_report(missing_optional_code)

    def _drop_policy_excluded(self, document: dict[str, YamlValue]) -> None:
        policy = self.mapping(document["policy"], "policy")
        self.sequence(
            policy["excluded_nonbaseline_codes"],
            "excluded_nonbaseline_codes",
        ).pop()

    def _drop_policy_conditional(self, document: dict[str, YamlValue]) -> None:
        policy = self.mapping(document["policy"], "policy")
        self.sequence(
            policy["conditional_required_codes"],
            "conditional_required_codes",
        ).pop()

    def _drop_probe_suite(self, document: dict[str, YamlValue]) -> None:
        summary = self.mapping(
            document["optional_probe_summary"],
            "optional_probe_summary",
        )
        self.sequence(summary["optional_suites"], "optional_suites").pop()

    def _drop_extension_suite(self, document: dict[str, YamlValue]) -> None:
        summary = self.mapping(
            document["optional_extension_summary"],
            "optional_extension_summary",
        )
        self.sequence(summary["optional_suites"], "optional_suites").pop()

    def _drop_uncovered(self, document: dict[str, YamlValue]) -> None:
        self.sequence(document["uncovered_codes"], "uncovered_codes").pop()

    def _invent_hard_gap(self, document: dict[str, YamlValue]) -> None:
        document["hard_gaps"] = ["AMBIGUOUS_DIMS"]

    def _invent_conditional_pending(self, document: dict[str, YamlValue]) -> None:
        document["conditional_pending"] = [
            {
                "code": "DETERMINISM_UNAVAILABLE",
                "precondition": "strict_unavailable_path",
            }
        ]

    def _drop_optional_unasserted(self, document: dict[str, YamlValue]) -> None:
        self.sequence(
            document["optional_emit_unasserted"],
            "optional_emit_unasserted",
        ).pop()

    def _drop_excluded(self, document: dict[str, YamlValue]) -> None:
        self.sequence(
            document["excluded_nonbaseline_codes"],
            "excluded_nonbaseline_codes",
        ).pop()


if __name__ == "__main__":
    unittest.main()

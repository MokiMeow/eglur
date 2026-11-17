"""Regression tests for the machine-readable diagnostic contract."""

import tempfile
import unittest
from pathlib import Path

import yaml

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]


from eglur_conformance.diagnostics import catalog as diagnostic_catalog
from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.yaml_document import YamlValue, load_yaml

CONTRACT_PATH = REPO_ROOT / diagnostic_contracts.DEFAULT_CONTRACT_PATH


class DiagnosticContractTests(unittest.TestCase):
    """Exercise contract loading and high-risk contract distinctions."""

    @staticmethod
    def _load_contract_source() -> dict[str, YamlValue]:
        return diagnostic_contracts.expect_mapping(
            load_yaml(CONTRACT_PATH),
            str(CONTRACT_PATH),
        )

    @staticmethod
    def _write_contract_source(path: Path, source: dict[str, YamlValue]) -> None:
        rendered = yaml.safe_dump(source, sort_keys=False)
        if not isinstance(rendered, str):
            raise TypeError("serialized diagnostic contract must be text")
        path.write_text(rendered, encoding="utf-8")

    @staticmethod
    def _related_spans(
        source: dict[str, YamlValue],
        example_index: int,
    ) -> list[YamlValue]:
        related = diagnostic_contracts.expect_mapping(
            source["related_span"],
            "related_span",
        )
        examples = diagnostic_contracts.expect_list(
            related["examples"],
            "related_span.examples",
        )
        example = diagnostic_contracts.expect_mapping(
            examples[example_index],
            f"related_span.examples[{example_index}]",
        )
        return diagnostic_contracts.expect_list(
            example["spans"],
            f"related_span.examples[{example_index}].spans",
        )

    @classmethod
    def _related_span(
        cls,
        source: dict[str, YamlValue],
        example_index: int,
        span_index: int,
    ) -> dict[str, YamlValue]:
        return diagnostic_contracts.expect_mapping(
            cls._related_spans(source, example_index)[span_index],
            f"related_span.examples[{example_index}].spans[{span_index}]",
        )

    @staticmethod
    def _source_span_examples(source: dict[str, YamlValue]) -> list[YamlValue]:
        source_span = diagnostic_contracts.expect_mapping(
            source["source_span"],
            "source_span",
        )
        return diagnostic_contracts.expect_list(
            source_span["examples"],
            "source_span.examples",
        )

    @classmethod
    def _source_span(
        cls,
        source: dict[str, YamlValue],
        example_index: int,
    ) -> dict[str, YamlValue]:
        example = diagnostic_contracts.expect_mapping(
            cls._source_span_examples(source)[example_index],
            f"source_span.examples[{example_index}]",
        )
        return diagnostic_contracts.expect_mapping(
            example["span"],
            f"source_span.examples[{example_index}].span",
        )

    @classmethod
    def _related_source_span(
        cls,
        source: dict[str, YamlValue],
        example_index: int,
        span_index: int,
    ) -> dict[str, YamlValue]:
        related = cls._related_span(source, example_index, span_index)
        return diagnostic_contracts.expect_mapping(
            related["span"],
            f"related_span.examples[{example_index}].spans[{span_index}].span",
        )

    def _assert_contract_rejected(
        self,
        source: dict[str, YamlValue],
        message: str,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "contracts.yaml"
            self._write_contract_source(path, source)

            with self.assertRaisesRegex(ValueError, message):
                diagnostic_contracts.load_contracts(path)

    def test_loads_complete_baseline_contract(self) -> None:
        document = diagnostic_contracts.load_contract_document(CONTRACT_PATH)
        contracts = document.codes

        catalog = diagnostic_catalog.load_catalog(CONTRACT_PATH.parent)
        self.assertEqual(set(contracts), catalog.codes)
        self.assertEqual(
            contracts["EFFECT_VIOLATION"].message_any_of,
            (("non-bang", "Fn", "pure"),),
        )
        self.assertEqual(
            contracts["TRAVERSE_RES_FAIL"].message_templates[0].pattern,
            "index {index}",
        )

    def test_loads_source_and_primary_span_contracts(self) -> None:
        document = diagnostic_contracts.load_contract_document(CONTRACT_PATH)
        source = document.source_span

        self.assertEqual(
            source.required_fields,
            (("kind", "source"), ("source", "Str"), ("bytes", "ByteRange")),
        )
        self.assertEqual(source.optional_fields, (("text", "TextRange"),))
        self.assertEqual(source.byte_range.start_base, 0)
        self.assertEqual(source.byte_range.unit, "physical_byte")
        self.assertEqual(source.byte_range.end_semantics, "half_open")
        self.assertEqual(source.text_range.coordinate.line_base, 1)
        self.assertEqual(source.text_range.coordinate.column_base, 1)
        self.assertEqual(
            source.text_range.coordinate.column_unit,
            "unicode_scalar",
        )
        self.assertEqual(source.text_range.leading_utf8_bom, "ignored")
        self.assertEqual(source.text_range.line_endings, "logical_lf")
        self.assertEqual(source.text_range.end_semantics, "half_open")
        self.assertEqual(source.source_keys, ("source", "bytes.start", "bytes.end"))
        self.assertIsNotNone(source.examples[0].text_range)
        self.assertIsNone(source.examples[1].text_range)
        self.assertEqual(document.primary_span.source_type, "SourceSpan")
        self.assertEqual(document.primary_span.unavailable_kind, "unavailable")

    def test_loads_related_span_contract_and_normative_examples(self) -> None:
        related = diagnostic_contracts.load_contract_document(
            CONTRACT_PATH
        ).related_span

        self.assertEqual(
            related.required_fields,
            (("span", "SourceSpan"), ("label", "Str")),
        )
        self.assertEqual(related.ordering.default, "source")
        self.assertTrue(related.ordering.code_specific_override)
        self.assertEqual(
            related.ordering.source_keys,
            ("span.source", "span.bytes.start", "span.bytes.end", "label"),
        )
        self.assertEqual(
            tuple(example.example_id for example in related.examples),
            ("trait-method-declaration", "ownership-path-causes"),
        )
        self.assertEqual(
            related.examples[0].spans[0].label, "trait method declared here"
        )
        self.assertEqual(
            related.examples[1].spans[1].span.byte_range.start,
            80,
        )

    def test_distinguishes_required_and_emit_if_present_data(self) -> None:
        contracts = diagnostic_contracts.load_contracts(CONTRACT_PATH)

        self.assertEqual(
            contracts["REDUCE_BY_PHASE_OVERLAP"].data_required,
            ("axis", "phase_index"),
        )
        self.assertEqual(
            contracts["ASSIGN_TARGET_NOT_ASSIGNABLE"].data_required,
            (),
        )
        self.assertEqual(
            contracts["ASSIGN_TARGET_NOT_ASSIGNABLE"].data_required_if_emitted,
            ("target_kind",),
        )

    def test_activates_conditional_data_from_exact_fixture_value(self) -> None:
        contract = diagnostic_contracts.load_contracts(CONTRACT_PATH)[
            "PACKAGE_LOCK_OUT_OF_SYNC"
        ]

        self.assertEqual(contract.required_data_keys({}), ("reason", "node"))
        self.assertEqual(
            contract.required_data_keys({"reason": "dependency_selector"}),
            ("reason", "node", "dependency"),
        )

    def test_type_diagnostic_data_preserves_semantic_partitions(self) -> None:
        contracts = diagnostic_contracts.load_contracts(CONTRACT_PATH)

        call = contracts["CALL_ARG_TYPE_MISMATCH"]
        self.assertEqual(call.data_required, ("reason", "actual_type"))
        self.assertEqual(
            call.required_data_keys({"reason": "parameter_type"}),
            ("reason", "actual_type", "parameter", "expected_type"),
        )
        self.assertEqual(
            call.required_data_keys({"reason": "type_family"}),
            ("reason", "actual_type", "expected_family"),
        )
        self.assertEqual(
            call.required_data_keys({"reason": "overload_no_match"}),
            ("reason", "actual_type", "candidates"),
        )

        expected_required = {
            "IF_BRANCH_TYPE_MISMATCH": ("left_type", "right_type"),
            "RETURN_TYPE_MISMATCH": ("expected_type", "actual_type"),
            "TRAIT_BOUND_NOT_SATISFIED": ("trait", "type"),
            "TRAIT_IMPL_SIGNATURE_MISMATCH": (
                "trait",
                "method",
                "expected_signature",
                "actual_signature",
            ),
            "TYPE_ARG_ARITY_MISMATCH": ("expected_count", "actual_count"),
        }
        for code, required in expected_required.items():
            with self.subTest(code=code):
                self.assertEqual(contracts[code].data_required, required)

        numeric_contract = contracts["NUMERIC_OPERAND_NOT_NUMERIC"]
        self.assertEqual(
            numeric_contract.required_data_keys({"reason": "unary_operand"}),
            ("reason", "operation", "actual_type"),
        )
        self.assertEqual(
            numeric_contract.required_data_keys({"reason": "binary_operand"}),
            ("reason", "operation", "left_type", "right_type"),
        )

        pattern_contract = contracts["PATTERN_TYPE_MISMATCH"]
        self.assertEqual(
            pattern_contract.required_data_keys({"reason": "structural_form"}),
            ("reason", "pattern_kind", "expected_type"),
        )
        self.assertEqual(
            pattern_contract.required_data_keys({"reason": "typed_pattern"}),
            ("reason", "pattern_kind", "expected_type", "actual_type"),
        )

        binding_contract = contracts["MATCH_BINDING_MISMATCH"]
        self.assertEqual(
            binding_contract.required_data_keys({"reason": "missing_binding"}),
            ("reason", "binding"),
        )
        self.assertEqual(
            binding_contract.required_data_keys({"reason": "binding_type"}),
            ("reason", "binding", "left_type", "right_type"),
        )

        property_contract = contracts["PROP_ACCESSOR_SIGNATURE_MISMATCH"]
        self.assertEqual(
            property_contract.required_data_keys({"reason": "signature_mismatch"}),
            (
                "property",
                "accessor",
                "reason",
                "actual_signature",
                "expected_signature",
            ),
        )
        self.assertEqual(
            property_contract.required_data_keys({"reason": "unexpected_accessor"}),
            (
                "property",
                "accessor",
                "reason",
                "actual_signature",
                "candidates",
            ),
        )

    def test_scalar_domain_preserves_kind_and_i64_bounds(self) -> None:
        accepted = ("value", True, -(2**63), 2**63 - 1)
        for value in accepted:
            with self.subTest(accepted=value):
                self.assertEqual(
                    diagnostic_contracts.expect_diagnostic_scalar(value, "scalar"),
                    value,
                )

        for value in (None, 1.0):
            with (
                self.subTest(wrong_type=value),
                self.assertRaisesRegex(TypeError, "must be Str, i64, or Bool"),
            ):
                diagnostic_contracts.expect_diagnostic_scalar(value, "scalar")

        for value in (-(2**63) - 1, 2**63, "\ud800"):
            with (
                self.subTest(invalid_value=value),
                self.assertRaisesRegex(ValueError, "must"),
            ):
                diagnostic_contracts.expect_diagnostic_scalar(value, "scalar")

        self.assertFalse(diagnostic_contracts.is_diagnostic_scalar(None))
        self.assertFalse(diagnostic_contracts.is_diagnostic_scalar(1.0))
        self.assertFalse(diagnostic_contracts.diagnostic_scalars_equal(True, 1))
        self.assertFalse(diagnostic_contracts.diagnostic_scalars_equal(False, 0))

    def test_rejects_non_diagnostic_scalar_contract_condition(self) -> None:
        mutations = (
            (None, TypeError, "must be Str, i64, or Bool"),
            (1.0, TypeError, "must be Str, i64, or Bool"),
            (-(2**63) - 1, ValueError, "must fit signed i64"),
            (2**63, ValueError, "must fit signed i64"),
        )
        for value, error_type, message in mutations:
            source = self._load_contract_source()
            codes = diagnostic_contracts.expect_mapping(source["codes"], "codes")
            contract = diagnostic_contracts.expect_mapping(
                codes["PACKAGE_LOCK_OUT_OF_SYNC"],
                "codes.PACKAGE_LOCK_OUT_OF_SYNC",
            )
            data = diagnostic_contracts.expect_mapping(
                contract["data"],
                "codes.PACKAGE_LOCK_OUT_OF_SYNC.data",
            )
            conditions = diagnostic_contracts.expect_list(
                data["conditional_required"],
                "codes.PACKAGE_LOCK_OUT_OF_SYNC.data.conditional_required",
            )
            condition = diagnostic_contracts.expect_mapping(
                conditions[0],
                "codes.PACKAGE_LOCK_OUT_OF_SYNC.data.conditional_required[0]",
            )
            when = diagnostic_contracts.expect_mapping(
                condition["when"],
                "codes.PACKAGE_LOCK_OUT_OF_SYNC.data.conditional_required[0].when",
            )
            when["equals"] = value
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "contracts.yaml"
                self._write_contract_source(path, source)
                with self.subTest(value=value):
                    try:
                        diagnostic_contracts.load_contracts(path)
                    except (TypeError, ValueError) as error:
                        self.assertIsInstance(error, error_type)
                        self.assertIn(message, str(error))
                    else:
                        self.fail("invalid diagnostic scalar was accepted")

    def test_rejects_related_span_schema_drift(self) -> None:
        source = self._load_contract_source()
        related = diagnostic_contracts.expect_mapping(
            source["related_span"],
            "related_span",
        )
        fields = diagnostic_contracts.expect_mapping(
            related["fields"],
            "related_span.fields",
        )
        required = diagnostic_contracts.expect_mapping(
            fields["required"],
            "related_span.fields.required",
        )
        required["span"] = "Path"

        self._assert_contract_rejected(source, "required fields")

    def test_rejects_nonpositive_related_span_coordinate(self) -> None:
        source = self._load_contract_source()
        span = self._related_source_span(source, 0, 0)
        text = diagnostic_contracts.expect_mapping(span["text"], "span.text")
        start = diagnostic_contracts.expect_mapping(text["start"], "span.text.start")
        start["line"] = 0

        self._assert_contract_rejected(source, "line must be a positive i64")

    def test_rejects_negative_or_reversed_source_byte_range(self) -> None:
        negative = self._load_contract_source()
        span = self._source_span(negative, 0)
        byte_range = diagnostic_contracts.expect_mapping(span["bytes"], "span.bytes")
        byte_range["start"] = -1
        self._assert_contract_rejected(negative, "start must be a non-negative i64")

        reversed_range = self._load_contract_source()
        span = self._source_span(reversed_range, 0)
        byte_range = diagnostic_contracts.expect_mapping(span["bytes"], "span.bytes")
        byte_range["end"] = 11
        self._assert_contract_rejected(
            reversed_range,
            "byte-range end must not precede start",
        )

    def test_rejects_incomplete_or_reversed_text_range(self) -> None:
        incomplete = self._load_contract_source()
        span = self._source_span(incomplete, 0)
        text = diagnostic_contracts.expect_mapping(span["text"], "span.text")
        del text["end"]
        self._assert_contract_rejected(incomplete, "keys must be")

        reversed_range = self._load_contract_source()
        span = self._related_source_span(reversed_range, 1, 0)
        text = diagnostic_contracts.expect_mapping(span["text"], "span.text")
        end = diagnostic_contracts.expect_mapping(text["end"], "span.text.end")
        end["line"] = 4
        self._assert_contract_rejected(
            reversed_range,
            "text-range end must not precede start",
        )

    def test_rejects_unsorted_related_span_example_sequence(self) -> None:
        source = self._load_contract_source()
        self._related_spans(source, 1).reverse()

        self._assert_contract_rejected(source, "default source order")

    def test_rejects_empty_related_span_label(self) -> None:
        source = self._load_contract_source()
        self._related_span(source, 0, 0)["label"] = ""

        self._assert_contract_rejected(source, "label must be a non-empty string")

    def test_rejects_unknown_contract_keys(self) -> None:
        source = self._load_contract_source()
        codes = diagnostic_contracts.expect_mapping(source["codes"], "codes")
        contract = diagnostic_contracts.expect_mapping(
            codes["EFFECT_VIOLATION"],
            "codes.EFFECT_VIOLATION",
        )
        message = diagnostic_contracts.expect_mapping(
            contract["message"],
            "codes.EFFECT_VIOLATION.message",
        )
        message["unknown"] = []
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "contracts.yaml"
            self._write_contract_source(path, source)

            with self.assertRaisesRegex(ValueError, "keys must be"):
                diagnostic_contracts.load_contracts(path)

    def test_rejects_unknown_contract_version(self) -> None:
        source = self._load_contract_source()
        source["version"] = "9.9"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "contracts.yaml"
            self._write_contract_source(path, source)

            with self.assertRaisesRegex(ValueError, r"version must be '0.1'"):
                diagnostic_contracts.load_contracts(path)


if __name__ == "__main__":
    unittest.main()

"""Boundary tests for front-end fixture loading and artifact persistence."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from eglur_conformance.artifact_content import JsonValue
from eglur_conformance.frontend import model, repository


def _syntax_case_payload() -> dict[str, JsonValue]:
    return {
        "version": "0.1",
        "cases": [
            {
                "id": "lex-case",
                "entrypoint": "tokens",
                "source": " 1\n",
                "expectation": {
                    "kind": "success",
                    "diagnostic_code": None,
                },
            }
        ],
    }


def _syntax_case_bytes(payload: dict[str, JsonValue]) -> bytes:
    return json.dumps(payload).encode()


class SyntaxCaseRepositoryTests(unittest.TestCase):
    """Require a strict boundary for authored focused syntax cases."""

    def test_valid_case_maps_to_closed_projection_input(self) -> None:
        """Decode one authored case into canonical projection input."""
        cases = repository.decode_syntax_cases(
            _syntax_case_bytes(_syntax_case_payload())
        )

        self.assertEqual(
            cases,
            (
                model.SyntaxCase(
                    "lex-case",
                    model.EntryPoint.TOKENS,
                    " 1\n",
                    model.FrontendExpectation(
                        model.OutcomeKind.SUCCESS,
                        None,
                        None,
                    ),
                ),
            ),
        )

    def test_cases_reject_duplicate_json_keys(self) -> None:
        """Reject ambiguous JSON before field decoding."""
        content = b'{"version":"0.1","version":"0.1","cases":[]}'

        with self.assertRaisesRegex(ValueError, "repeats key"):
            repository.decode_syntax_cases(content)

    def test_cases_reject_duplicate_case_ids(self) -> None:
        """Keep syntax-case identity globally unique before corpus merging."""
        payload = _syntax_case_payload()
        cases = payload["cases"]
        self.assertIsInstance(cases, list)
        cases.append(cases[0])

        with self.assertRaisesRegex(ValueError, "case ids must be unique"):
            repository.decode_syntax_cases(_syntax_case_bytes(payload))

    def test_cases_reject_unknown_fields_and_wrong_phase_codes(self) -> None:
        """Fail closed on schema drift and phase-inconsistent diagnostics."""
        unknown = _syntax_case_payload()
        cases = unknown["cases"]
        self.assertIsInstance(cases, list)
        case = cases[0]
        self.assertIsInstance(case, dict)
        case["unknown"] = True
        with self.assertRaisesRegex(ValueError, "unsupported"):
            repository.decode_syntax_cases(_syntax_case_bytes(unknown))

        wrong_phase = _syntax_case_payload()
        cases = wrong_phase["cases"]
        self.assertIsInstance(cases, list)
        case = cases[0]
        self.assertIsInstance(case, dict)
        expectation = case["expectation"]
        self.assertIsInstance(expectation, dict)
        expectation["kind"] = "lexical_failure"
        expectation["diagnostic_code"] = "PARSE_UNEXPECTED_TOKEN"
        with self.assertRaisesRegex(ValueError, "must start with LEX_"):
            repository.decode_syntax_cases(_syntax_case_bytes(wrong_phase))

    def test_cases_require_a_focused_entrypoint(self) -> None:
        """Keep full compilation units in the conformance-owned input path."""
        payload = _syntax_case_payload()
        cases = payload["cases"]
        self.assertIsInstance(cases, list)
        case = cases[0]
        self.assertIsInstance(case, dict)
        case["entrypoint"] = "compilation_unit"

        with self.assertRaisesRegex(ValueError, "focused front-end entrypoint"):
            repository.decode_syntax_cases(_syntax_case_bytes(payload))

    def test_token_cases_reject_parser_failures(self) -> None:
        """Keep outcome phases compatible with the selected entry point."""
        payload = _syntax_case_payload()
        cases = payload["cases"]
        self.assertIsInstance(cases, list)
        case = cases[0]
        self.assertIsInstance(case, dict)
        expectation = case["expectation"]
        self.assertIsInstance(expectation, dict)
        expectation["kind"] = "parse_failure"
        expectation["diagnostic_code"] = "PARSE_UNEXPECTED_TOKEN"

        with self.assertRaisesRegex(ValueError, "cannot carry a parser failure"):
            repository.decode_syntax_cases(_syntax_case_bytes(payload))


class CorpusPersistenceTests(unittest.TestCase):
    """Verify stale detection and atomic generated-artifact replacement."""

    def test_current_check_compares_exact_bytes_and_handles_missing_output(
        self,
    ) -> None:
        """Treat absent or byte-different generated output as stale."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / repository.CORPUS_PATH
            with mock.patch.object(
                repository,
                "generated_corpus_bytes",
                return_value=b"current\n",
            ):
                self.assertFalse(repository.corpus_is_current(root))
                output.parent.mkdir(parents=True)
                output.write_bytes(b"stale\n")
                self.assertFalse(repository.corpus_is_current(root))
                output.write_bytes(b"current\n")
                self.assertTrue(repository.corpus_is_current(root))

    def test_write_is_atomic_and_leaves_no_temporary_artifact(self) -> None:
        """Replace the generated file through a same-directory temporary file."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(
                repository,
                "generated_corpus_bytes",
                return_value=b"generated\n",
            ):
                repository.write_corpus(root)

            output = root / repository.CORPUS_PATH
            self.assertEqual(output.read_bytes(), b"generated\n")
            self.assertEqual(
                tuple(output.parent.glob(f".{output.name}.*.tmp")),
                (),
            )


if __name__ == "__main__":
    unittest.main()

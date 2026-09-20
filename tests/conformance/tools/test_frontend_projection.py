"""Contract tests for syntax-front-end corpus projection."""

import json
import unittest
from collections import Counter
from dataclasses import replace
from pathlib import Path
from typing import ClassVar

from eglur_conformance.frontend import codec, model, projection, repository
from eglur_conformance.suite import model as suite_model

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parents[2]
TEST_DIGEST = f"sha256:{'0' * 64}"


def _success_expectation() -> model.FrontendExpectation:
    return model.FrontendExpectation(model.OutcomeKind.SUCCESS, None, None)


def _test_case(
    source: suite_model.FixtureSource,
    outcome: suite_model.ExpectedOutcome,
    *,
    registry_acquisitions: tuple[suite_model.RegistryAcquisition, ...] = (),
) -> suite_model.TestCase:
    return suite_model.TestCase(
        test_id="TEST-case",
        category="test",
        title="test case",
        invocation=suite_model.FixtureInvocation(
            source=source,
            registry_acquisitions=registry_acquisitions,
            mode="auto",
            runner="main",
        ),
        expectation=suite_model.Expectation(
            outcome=outcome,
            value_diagnostics=(),
            diagnostic_contains=None,
            side_effects=(),
            process_exit=None,
        ),
    )


def _bundle(
    test_case: suite_model.TestCase,
    *,
    required: bool = True,
) -> suite_model.ConformanceBundle:
    participation: suite_model.SuiteParticipation
    if required:
        participation = suite_model.BaselineParticipation()
    else:
        participation = suite_model.OptionalParticipation()
    entry = suite_model.SuiteEntry("test-suite", "core/test.yaml", participation)
    return suite_model.ConformanceBundle(
        manifest=suite_model.Manifest(
            version="0.1",
            release_history_root="0" * 40,
            release="v0.1.0",
            status="draft",
            root="tests/conformance",
            suites=(entry,),
        ),
        suites=(
            suite_model.ConformanceSuite(
                entry,
                suite_model.SuiteDocument((test_case,)),
            ),
        ),
    )


class FrontendCorpusTests(unittest.TestCase):
    """Verify the complete repository projection and checked-in artifact."""

    corpus: ClassVar[model.FrontendCorpus]
    artifact_bytes: ClassVar[bytes]

    @classmethod
    def setUpClass(cls) -> None:
        """Build and decode the complete current corpus once."""
        cls.corpus = repository.build_corpus(REPO_ROOT)
        cls.artifact_bytes = (REPO_ROOT / repository.CORPUS_PATH).read_bytes()

    def test_complete_projection_matches_the_source_unit_census(self) -> None:
        """Cover every authoritative source form without merging identities."""
        origin_counts = Counter(type(case.origin) for case in self.corpus.cases)
        source_counts = Counter(type(case.source) for case in self.corpus.cases)

        self.assertEqual(len(self.corpus.cases), 2140)
        self.assertEqual(origin_counts[model.InlineConformanceOrigin], 1760)
        self.assertEqual(origin_counts[model.FileConformanceOrigin], 323)
        self.assertEqual(origin_counts[model.RegistryConformanceOrigin], 4)
        self.assertEqual(origin_counts[model.SyntaxCaseOrigin], 53)
        self.assertEqual(source_counts[model.Utf8Text], 2139)
        self.assertEqual(source_counts[model.RawBytes], 1)

    def test_entrypoints_and_public_provenance_remain_independent_axes(self) -> None:
        """Keep entry points and public links independent of source identity."""
        entrypoint_counts = Counter(case.entrypoint for case in self.corpus.cases)
        public_examples = tuple(
            example for case in self.corpus.cases for example in case.public_examples
        )

        self.assertEqual(entrypoint_counts[model.EntryPoint.COMPILATION_UNIT], 2087)
        self.assertEqual(entrypoint_counts[model.EntryPoint.TOKENS], 7)
        self.assertEqual(entrypoint_counts[model.EntryPoint.EXPRESSION], 29)
        self.assertEqual(entrypoint_counts[model.EntryPoint.EXPRESSION_SEQUENCE], 3)
        self.assertEqual(entrypoint_counts[model.EntryPoint.CONTROL_HEAD], 14)
        self.assertEqual(len(public_examples), 5)
        self.assertEqual(len(set(public_examples)), 5)

    def test_later_phase_errors_are_front_end_successes(self) -> None:
        """Do not turn semantic or feature diagnostics into parser failures."""
        expectations = {
            case.origin.test_id: case.expectation.kind
            for case in self.corpus.cases
            if isinstance(case.origin, model.InlineConformanceOrigin)
        }

        self.assertIs(
            expectations["PAR-var-local-declaration-uses-stable-forbidden-code"],
            model.OutcomeKind.SUCCESS,
        )
        self.assertIs(
            expectations["TRT-dyn-trait-must-be-explicit"],
            model.OutcomeKind.SUCCESS,
        )

    def test_invalid_utf8_remains_exact_raw_lexical_input(self) -> None:
        """Preserve the pre-tokenization byte failure without text coercion."""
        raw_cases = [
            case
            for case in self.corpus.cases
            if isinstance(case.source, model.RawBytes)
        ]

        self.assertEqual(len(raw_cases), 1)
        source = raw_cases[0].source
        self.assertIsInstance(source, model.RawBytes)
        if not isinstance(source, model.RawBytes):
            self.fail("raw source did not retain its byte representation")
        self.assertEqual(source.content, b"\xff")
        self.assertIs(
            raw_cases[0].expectation.kind,
            model.OutcomeKind.LEXICAL_FAILURE,
        )
        self.assertEqual(
            raw_cases[0].expectation.diagnostic_code,
            "LEX_INVALID_UTF8",
        )
        span = raw_cases[0].expectation.primary_span
        self.assertIsInstance(span, model.ByteSpanExpectation)
        if not isinstance(span, model.ByteSpanExpectation):
            self.fail("raw source did not retain its exact byte-span assertion")
        self.assertEqual(span.source, "code")
        self.assertEqual(span.byte_range.start, 0)
        self.assertEqual(span.byte_range.end, 1)

    def test_checked_in_artifact_is_current_and_round_trips_exactly(self) -> None:
        """Keep the derived artifact byte-exact and strictly decodable."""
        decoded = codec.decode_corpus_bytes(self.artifact_bytes)

        self.assertEqual(decoded, self.corpus)
        self.assertEqual(codec.encode_corpus(decoded), self.artifact_bytes)
        self.assertTrue(repository.corpus_is_current(REPO_ROOT))


class FrontendModelAndCodecTests(unittest.TestCase):
    """Reject invalid axis combinations and malformed persisted data."""

    def case(self) -> model.FrontendCase:
        """Return one valid case for model mutations."""
        return model.FrontendCase(
            origin=model.InlineConformanceOrigin("suite", "TEST-case"),
            entrypoint=model.EntryPoint.COMPILATION_UNIT,
            source=model.Utf8Text("fn main() -> Unit {}\n"),
            expectation=_success_expectation(),
        )

    def test_success_cannot_carry_a_diagnostic(self) -> None:
        """Keep acceptance distinct from lexical and parse failure records."""
        with self.assertRaisesRegex(ValueError, "success cannot carry"):
            model.FrontendExpectation(
                model.OutcomeKind.SUCCESS,
                "PARSE_UNEXPECTED_TOKEN",
                None,
            )

    def test_failure_code_must_match_its_phase(self) -> None:
        """Reject cross-phase diagnostic classification."""
        with self.assertRaisesRegex(ValueError, "must start with LEX_"):
            model.FrontendExpectation(
                model.OutcomeKind.LEXICAL_FAILURE,
                "PARSE_UNEXPECTED_TOKEN",
                None,
            )

    def test_public_provenance_requires_an_inline_origin(self) -> None:
        """Prevent public links from creating a second file-set identity."""
        with self.assertRaisesRegex(ValueError, "only inline"):
            model.FrontendCase(
                origin=model.FileConformanceOrigin("suite", "TEST-case", "main.eglur"),
                entrypoint=model.EntryPoint.COMPILATION_UNIT,
                source=model.Utf8Text("fn main() -> Unit {}\n"),
                expectation=_success_expectation(),
                public_examples=(model.PublicExample("README.md", 1),),
            )

    def test_corpus_rejects_duplicate_and_unsorted_identities(self) -> None:
        """Make source-unit identity the sole ordering and deduplication key."""
        first = self.case()
        second = replace(
            first,
            origin=model.InlineConformanceOrigin("suite", "A-case"),
        )

        with self.assertRaisesRegex(ValueError, "must be sorted"):
            model.FrontendCorpus("0.1", TEST_DIGEST, (first, second))
        with self.assertRaisesRegex(ValueError, "globally unique"):
            model.FrontendCorpus("0.1", TEST_DIGEST, (first, first))

    def test_codec_rejects_duplicate_keys_and_unknown_fields(self) -> None:
        """Keep the JSON artifact closed at every mapping boundary."""
        duplicate = b'{"schema_version":"0.1","schema_version":"0.1"}'
        with self.assertRaisesRegex(ValueError, "repeats key"):
            codec.decode_corpus_bytes(duplicate)

        corpus = model.FrontendCorpus("0.1", TEST_DIGEST, (self.case(),))
        payload = codec.corpus_payload(corpus)
        payload["unsupported"] = True
        with self.assertRaisesRegex(ValueError, "unsupported"):
            codec.decode_corpus_bytes(json.dumps(payload).encode())

    def test_codec_rejects_noncanonical_raw_byte_hex(self) -> None:
        """Prevent alternate persisted representations of the same bytes."""
        case = replace(self.case(), source=model.RawBytes(b"\xff"))
        corpus = model.FrontendCorpus("0.1", TEST_DIGEST, (case,))
        payload = codec.corpus_payload(corpus)
        raw_case = payload["cases"]
        self.assertIsInstance(raw_case, list)
        source = raw_case[0]["source"]
        self.assertIsInstance(source, dict)
        source["hex"] = "FF"

        with self.assertRaisesRegex(ValueError, "canonical lowercase"):
            codec.decode_corpus_bytes(json.dumps(payload).encode())


class FrontendProjectionEdgeTests(unittest.TestCase):
    """Exercise ambiguous and unsupported source projection paths."""

    def test_file_set_failure_requires_an_owned_source_path(self) -> None:
        """Do not guess which file owns a front-end failure."""
        diagnostic = suite_model.DiagnosticAssertion(
            code="PARSE_UNEXPECTED_TOKEN",
            message_all_of=(),
            message_any_of=(),
            data_keys=(),
            data_equals=(),
            help_contains=None,
            related_contains=None,
            primary_span=None,
        )
        source = suite_model.FileSetSource(
            files=(suite_model.SourceFile("main.eglur", "@"),),
            symlinks=(),
            entry="main.eglur",
            root=None,
        )

        with self.assertRaisesRegex(ValueError, "must identify one canonical"):
            projection.project_corpus(
                _bundle(
                    _test_case(source, suite_model.CompileErrorOutcome(diagnostic))
                ),
                (),
                (),
                TEST_DIGEST,
            )

    def test_repeated_registry_source_bytes_are_not_materialized(self) -> None:
        """Reject compact provider values rather than guessing parser input."""
        acquisition = suite_model.RegistryAcquisition(
            package="registry.example/pkg",
            version="1.0.0",
            provider_steps=(
                suite_model.RegistryEmitStep(
                    suite_model.RegistryFileEntry(
                        b"src/lib.eglur",
                        suite_model.RegistryRepeatedByte(ord("x"), 4),
                    )
                ),
                suite_model.RegistryFinishStep("complete"),
            ),
        )
        outcome = suite_model.ValueOutcome("Ok", None, None, None)

        with self.assertRaisesRegex(TypeError, "unexpanded repeated source bytes"):
            projection.project_corpus(
                _bundle(
                    _test_case(
                        suite_model.Utf8Source("fn main() -> Unit {}\n"),
                        outcome,
                        registry_acquisitions=(acquisition,),
                    )
                ),
                (),
                (),
                TEST_DIGEST,
            )

    def test_syntax_case_projects_its_explicit_front_end_expectation(self) -> None:
        """Do not reclassify focused cases from source spelling or category."""
        source = "schema.reduce_by()"
        test_case = _test_case(
            suite_model.Utf8Source("fn main() -> Unit {}\n"),
            suite_model.ValueOutcome("Unit", None, None, None),
        )
        corpus = projection.project_corpus(
            _bundle(test_case),
            (
                model.SyntaxCase(
                    "reduce-invalid",
                    model.EntryPoint.EXPRESSION,
                    source,
                    _success_expectation(),
                ),
            ),
            (),
            TEST_DIGEST,
        )
        syntax_case = next(
            case
            for case in corpus.cases
            if isinstance(case.origin, model.SyntaxCaseOrigin)
        )

        self.assertIs(syntax_case.expectation.kind, model.OutcomeKind.SUCCESS)
        self.assertIsNone(syntax_case.expectation.diagnostic_code)

    def test_public_links_require_a_unique_required_inline_fixture(self) -> None:
        """Reject optional fixture links and exact source drift."""
        source = "fn main() -> Unit {}\n"
        test_case = _test_case(
            suite_model.Utf8Source(source),
            suite_model.ValueOutcome("Unit", None, None, None),
        )
        binding = model.PublicExampleBinding("TEST-case", "README.md", 1, source)

        with self.assertRaisesRegex(ValueError, "does not name"):
            projection.project_corpus(
                _bundle(test_case, required=False),
                (),
                (binding,),
                TEST_DIGEST,
            )

        with self.assertRaisesRegex(ValueError, "source drift"):
            projection.project_corpus(
                _bundle(test_case),
                (),
                (replace(binding, source="different\n"),),
                TEST_DIGEST,
            )


if __name__ == "__main__":
    unittest.main()

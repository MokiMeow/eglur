"""Contract tests for syntax-front-end corpus projection."""

import json
import unittest
from dataclasses import replace
from pathlib import Path
from typing import ClassVar

from eglur_conformance import markdown_document
from eglur_conformance.documentation import repository as documentation_repository
from eglur_conformance.frontend import codec, model, projection, repository
from eglur_conformance.suite import model as suite_model
from eglur_conformance.suite import repository as suite_repository

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

    def test_complete_projection_preserves_source_unit_identities(self) -> None:
        """Project every authored source exactly once without changing its payload."""
        bundle = suite_repository.load_conformance_inputs(REPO_ROOT).bundle
        expected: dict[model.FrontendOrigin, model.FrontendSource] = {}
        for test in bundle.tests:
            suite_id, test_id = test.identity.suite_id, test.identity.test_id
            source = test.case.invocation.source
            if isinstance(source, suite_model.Utf8Source):
                expected[model.InlineConformanceOrigin(suite_id, test_id)] = (
                    model.Utf8Text(source.text)
                )
            elif isinstance(source, suite_model.RawBytesSource):
                expected[model.InlineConformanceOrigin(suite_id, test_id)] = (
                    model.RawBytes(bytes.fromhex(source.hex_bytes))
                )
            else:
                for item in source.files:
                    if item.path.endswith(".eglur"):
                        expected[
                            model.FileConformanceOrigin(suite_id, test_id, item.path)
                        ] = model.Utf8Text(item.text)

            if isinstance(
                test.case.expectation.outcome, suite_model.CompileErrorOutcome
            ):
                continue
            for acquisition in test.case.invocation.registry_acquisitions:
                finish = acquisition.provider_steps[-1]
                if (
                    not isinstance(finish, suite_model.RegistryFinishStep)
                    or finish.outcome != "complete"
                ):
                    continue
                for step in acquisition.provider_steps[:-1]:
                    if not isinstance(step, suite_model.RegistryEmitStep):
                        continue
                    entry = step.entry
                    if (
                        not isinstance(entry, suite_model.RegistryFileEntry)
                        or not isinstance(entry.path, bytes)
                        or not entry.path.endswith(b".eglur")
                    ):
                        continue
                    content = entry.content
                    if not isinstance(content, bytes):
                        raise TypeError(
                            "an admitted registry source must have exact bytes"
                        )
                    origin = model.RegistryConformanceOrigin(
                        suite_id,
                        test_id,
                        acquisition.package,
                        acquisition.version,
                        entry.path.decode("utf-8"),
                    )
                    self.assertNotIn(origin, expected)
                    expected[origin] = model.Utf8Text(content.decode("utf-8"))

        syntax_cases = repository.decode_syntax_cases(
            (REPO_ROOT / "tests/frontend/syntax-cases.json").read_bytes()
        )
        for case in syntax_cases:
            expected[model.SyntaxCaseOrigin(case.case_id)] = model.Utf8Text(case.source)

        self.assertEqual(
            sorted(
                (case.origin for case in self.corpus.cases), key=model.origin_identity
            ),
            sorted(expected, key=model.origin_identity),
        )
        self.assertEqual(
            {case.origin: case.source for case in self.corpus.cases}, expected
        )

    def test_entrypoints_and_public_provenance_remain_independent_axes(self) -> None:
        """Retain declared entrypoints and every exact public fixture binding."""
        syntax_cases = repository.decode_syntax_cases(
            (REPO_ROOT / "tests/frontend/syntax-cases.json").read_bytes()
        )
        syntax_entrypoints = {case.case_id: case.entrypoint for case in syntax_cases}
        actual_links: list[tuple[str, int, str, str]] = []
        for case in self.corpus.cases:
            if isinstance(case.origin, model.SyntaxCaseOrigin):
                self.assertIs(case.entrypoint, syntax_entrypoints[case.origin.case_id])
            else:
                self.assertIs(case.entrypoint, model.EntryPoint.COMPILATION_UNIT)
            if case.public_examples:
                origin, source = case.origin, case.source
                if not isinstance(
                    origin, model.InlineConformanceOrigin
                ) or not isinstance(source, model.Utf8Text):
                    raise TypeError(
                        "public examples must bind to inline UTF-8 fixtures"
                    )
                test_id, text = origin.test_id, source.text
                actual_links.extend(
                    (
                        example.document,
                        example.opening_line,
                        test_id,
                        text,
                    )
                    for example in case.public_examples
                )

        expected_links: list[tuple[str, int, str, str]] = []
        documents = documentation_repository.load_tracked_documentation(REPO_ROOT)
        for source in documents.sources:
            for block in markdown_document.fenced_code_blocks(REPO_ROOT / source.path):
                if block.info.startswith("eglur fixture="):
                    expected_links.append(
                        (
                            source.path.as_posix(),
                            block.opening_line,
                            block.info.removeprefix("eglur fixture="),
                            block.code,
                        )
                    )
        self.assertEqual(sorted(actual_links), sorted(expected_links))

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
        raw_case = next(
            case
            for case in self.corpus.cases
            if case.origin
            == model.InlineConformanceOrigin(
                "core-lexer",
                "LEX-invalid-utf8-source-is-rejected-before-tokenization",
            )
        )
        source = raw_case.source
        self.assertIsInstance(source, model.RawBytes)
        if not isinstance(source, model.RawBytes):
            self.fail("raw source did not retain its byte representation")
        self.assertEqual(source.content, b"\xff")
        self.assertIs(
            raw_case.expectation.kind,
            model.OutcomeKind.LEXICAL_FAILURE,
        )
        self.assertEqual(
            raw_case.expectation.diagnostic_code,
            "LEX_INVALID_UTF8",
        )
        span = raw_case.expectation.primary_span
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

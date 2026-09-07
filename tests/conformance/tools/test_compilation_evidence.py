"""Adversarial tests for phase-specific compiler evidence."""

import copy
import unittest
from collections.abc import Callable, ItemsView, Iterator, Mapping
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Self, cast

import yaml

TOOLS_DIR = Path(__file__).resolve().parent


from eglur_conformance import artifact_content
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.evidence.compilation import codec as compile_codec
from eglur_conformance.evidence.compilation import model as compile_model
from eglur_conformance.evidence.compilation import policy as compile_policy
from eglur_conformance.evidence.compilation import projection as compile_projection
from eglur_conformance.evidence.execution_context import codec as context_codec
from eglur_conformance.evidence.execution_context import model as context_model
from eglur_conformance.suite import artifact as suite_artifact
from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import model as suite_model
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue, load_yaml_text

SPECIFICATION_COMMIT = "0123456789abcdef0123456789abcdef01234567"
RUN_ID = "run-2026-07-30-compile-001"


@dataclass(frozen=True)
class EvidenceBundle:
    """One complete synthetic compile-evidence input."""

    manifest_bytes: bytes
    suite_document_bytes: Mapping[str, bytes]
    context: context_model.ExecutionContext
    observations: tuple[compile_model.CompileObservation, ...]
    raw_artifacts: tuple[bytes, ...]
    signoff_bytes: bytes
    binding: compile_model.CompileEvidenceBinding


class SwitchingSuiteDocuments(Mapping[str, bytes]):
    """Expose one suite mapping on first traversal and another thereafter."""

    def __init__(
        self,
        first: Mapping[str, bytes],
        later: Mapping[str, bytes],
    ) -> None:
        self._first = first
        self._later = later
        self.items_calls = 0

    def __getitem__(self, key: str) -> bytes:
        return self._later[key]

    def __iter__(self) -> Iterator[str]:
        return iter(self._later)

    def __len__(self) -> int:
        return len(self._later)

    def items(self) -> ItemsView[str, bytes]:
        self.items_calls += 1
        selected = self._first if self.items_calls == 1 else self._later
        return selected.items()


class SplitViewBytes(bytes):
    """Expose forged decoded text while retaining different buffer bytes."""

    decoded_text: str

    def __new__(cls, content: bytes, decoded_text: str) -> Self:
        instance = super().__new__(cls, content)
        instance.decoded_text = decoded_text
        return instance

    def decode(
        self,
        encoding: str = "utf-8",
        errors: str = "strict",
    ) -> str:
        """Return text unrelated to the immutable buffer."""
        return self.decoded_text


def context_artifact(
    active_preconditions: tuple[str, ...],
) -> dict[str, object]:
    """Return one complete compiler execution context."""
    return {
        "kind": "eglur_execution_context",
        "schema_version": "0.1",
        "context": {
            "compiler": {
                "name": "egc",
                "version": "0.1.0",
                "revision": ("git:fedcba9876543210fedcba9876543210fedcba98"),
            },
            "runtime": {
                "name": "eglur-runtime",
                "version": "0.1.0",
                "revision": ("git:0123456789abcdef0123456789abcdef01234567"),
            },
            "target": {"triple": "x86_64-unknown-linux-gnu"},
            "backend": {
                "applicability": "identified",
                "name": "cpu-reference",
                "version": "1.0.0",
                "revision": f"sha256:{'a' * 64}",
            },
            "device": {
                "applicability": "identified",
                "kind": "cpu",
                "vendor": "GenuineIntel",
                "model": "reference-model",
                "ordinal": 0,
            },
            "driver_runtime": {
                "applicability": "identified",
                "name": "glibc",
                "version": "2.39",
                "revision": f"sha256:{'b' * 64}",
            },
            "determinism": {
                "default_profile": "strict",
                "randomness": {"mode": "unused"},
            },
            "parallelism": {
                "workers": 1,
                "threads": 1,
                "processes": 1,
            },
            "active_preconditions": list(active_preconditions),
        },
    }


def yaml_bytes(value: YamlValue) -> bytes:
    """Serialize one synthetic YAML document as UTF-8 bytes."""
    rendered = yaml.safe_dump(
        value,
        allow_unicode=True,
        sort_keys=False,
    )
    if not isinstance(rendered, str):
        raise TypeError("synthetic YAML serialization must return text")
    return rendered.encode("utf-8")


def synthetic_inventory_artifacts() -> tuple[bytes, dict[str, bytes]]:
    """Return positive, negative, conditional, and optional fixture suites."""
    manifest: YamlValue = {
        "version": "0.1",
        "release_history_root": "a" * 40,
        "release": "v0.1.0",
        "status": "draft",
        "root": "tests/conformance",
        "suites": [
            {
                "id": "core-synthetic",
                "file": "core/synthetic.yaml",
                "required": True,
            },
            {
                "id": "core-conditional",
                "file": "core/conditional.yaml",
                "required": True,
                "precondition": "ffi_linkage_probe",
            },
            {
                "id": "optional-synthetic",
                "file": "optional/synthetic.yaml",
                "required": False,
            },
        ],
    }
    suites: dict[str, YamlValue] = {
        "core/synthetic.yaml": {
            "tests": [
                {
                    "id": "SYN-positive",
                    "category": "synthetic",
                    "title": "Synthetic positive compile fixture",
                    "code": (
                        "fn main!() -> Res[Str, Str] { "
                        'yield Ok("compile_error is only text") }\n'
                    ),
                    "expect": {
                        "value": {
                            "kind": "Ok",
                            "repr": '"compile_error is only text"',
                        }
                    },
                },
                {
                    "id": "SYN-negative",
                    "category": "synthetic",
                    "title": "Synthetic negative compile fixture",
                    "code": "let x = missing // @primary missing-name\n",
                    "expect": {
                        "compile_error": {
                            "code": "NAME_NOT_FOUND",
                            "message_contains": ["name"],
                            "message_contains_any": [["missing", "unresolved"]],
                            "data_keys": ["name"],
                            "data_equals": {"name": "missing"},
                            "primary_span": {"marker": "missing-name"},
                        }
                    },
                },
            ]
        },
        "core/conditional.yaml": {
            "tests": [
                {
                    "id": "SYN-conditional-positive",
                    "category": "synthetic",
                    "title": "Synthetic conditional compile fixture",
                    "code_bytes_hex": "666e206d61696e2829202d3e20556e6974203d202829",
                    "expect": {
                        "value": {
                            "kind": "Unit",
                            "repr": "()",
                        }
                    },
                }
            ]
        },
        "optional/synthetic.yaml": {
            "tests": [
                {
                    "id": "SYN-optional",
                    "category": "diag_withdiag",
                    "title": "Synthetic optional compile fixture",
                    "code": "missing\n",
                    "expect": {
                        "compile_error": {
                            "code": "NAME_NOT_FOUND",
                            "message_contains": ["missing"],
                            "data_keys": [],
                            "help_contains": ["optional"],
                            "related_contains": ["optional"],
                        }
                    },
                }
            ]
        },
    }
    return yaml_bytes(manifest), {
        path: yaml_bytes(document) for path, document in suites.items()
    }


def spec_signoff_bytes(
    conformance_bundle_digest: str,
    *,
    release: str = "v0.1.0",
    commit: str = SPECIFICATION_COMMIT,
) -> bytes:
    """Return one minimally projected successful spec-package sign-off."""
    return (
        "kind: spec_package_release\n"
        "specification:\n"
        f"  release: {release}\n"
        f"  commit: {commit}\n"
        f"conformance_bundle_digest: {conformance_bundle_digest}\n"
        "implementation_conformance: not_evaluated\n"
        "verdict: GO\n"
    ).encode()


def observed_diagnostic(
    expected: compile_projection.ExpectedDiagnostic,
) -> compile_model.ObservedDiagnostic:
    """Return one diagnostic satisfying every portable fixture assertion."""
    tokens = [
        *expected.message_all_of,
        *(group[0] for group in expected.message_any_of),
    ]
    message = " ".join(tokens) if tokens else expected.code
    exact_values = dict(expected.data_equals)
    data = tuple(
        (
            key,
            exact_values.get(
                key,
                compile_model.DiagnosticScalar(
                    kind="str",
                    value=f"observed-{key}",
                ),
            ),
        )
        for key in expected.data_keys
    )
    expected_span = expected.primary_span
    if expected_span is None:
        primary_span: diagnostic_model.PrimarySpan = diagnostic_model.UnavailableSpan()
    elif isinstance(expected_span, suite_model.MarkerPrimarySpan):
        coordinate = diagnostic_model.SourceCoordinate(expected_span.line, 1)
        primary_span = diagnostic_model.SourceSpan(
            source=expected_span.source,
            byte_range=diagnostic_model.ByteRange(0, 0),
            text_range=diagnostic_model.TextRange(
                start=coordinate,
                end=coordinate,
            ),
        )
    else:
        primary_span = diagnostic_model.SourceSpan(
            source=expected_span.source,
            byte_range=expected_span.byte_range,
        )
    return compile_model.ObservedDiagnostic(
        severity="error",
        code=expected.code,
        message=message,
        data=data,
        primary_span=primary_span,
    )


def observation_for_fixture(
    fixture: compile_projection.CompileFixture,
    context_digest: str,
) -> tuple[compile_model.CompileObservation, tuple[bytes, bytes]]:
    """Return one matching observation and its raw compiler/log bytes."""
    log = f"compiler-log:{fixture.identity}".encode()
    expected = fixture.expected_diagnostic
    outcome = (
        compile_model.CompileSucceeded()
        if expected is None
        else compile_model.CompileRejected(
            primary_diagnostic=observed_diagnostic(expected),
        )
    )
    compiler_result = artifact_content.canonical_json_bytes(
        compile_codec.compiler_result_payload(
            compile_model.CompilerResult(outcome=outcome)
        )
    )
    return (
        compile_model.CompileObservation(
            suite_id=fixture.identity.suite_id,
            test_id=fixture.identity.test_id,
            specification_commit=SPECIFICATION_COMMIT,
            run_id=RUN_ID,
            context_digest=context_digest,
            input_digest=fixture.input_digest,
            compiler_result_digest=(
                artifact_content.raw_artifact_digest(compiler_result)
            ),
            log_digest=artifact_content.raw_artifact_digest(log),
            termination=compile_model.ExitedProcess(
                code=0 if outcome.kind == "succeeded" else 1,
            ),
            outcome=outcome,
        ),
        (compiler_result, log),
    )


def rebind(
    binding: compile_model.CompileEvidenceBinding,
    observations: tuple[compile_model.CompileObservation, ...],
) -> compile_model.CompileEvidenceBinding:
    """Return a binding for the exact supplied observation addresses."""
    return replace(
        binding,
        observation_digests=tuple(
            sorted(
                compile_codec.compile_observation_digest(observation)
                for observation in observations
            )
        ),
    )


def build_bundle(
    manifest_bytes: bytes,
    suite_document_bytes: dict[str, bytes],
    *,
    active_preconditions: tuple[str, ...] = ("ffi_linkage_probe",),
) -> EvidenceBundle:
    """Build matching observations for every active required fixture."""
    context = context_codec.parse_execution_context(
        context_artifact(active_preconditions)
    )
    context_digest = context_codec.execution_context_digest(context)
    conformance_bundle = suite_artifact.parse_conformance_bundle_bytes(
        manifest_bytes,
        suite_document_bytes,
    )
    inventory = compile_projection.conformance_compile_inventory(conformance_bundle)
    active_fixtures = tuple(
        fixture
        for fixture in inventory.fixtures
        if fixture.precondition is None
        or fixture.precondition in context.active_preconditions
    )
    generated = tuple(
        observation_for_fixture(fixture, context_digest) for fixture in active_fixtures
    )
    observations = tuple(item[0] for item in generated)
    raw_artifacts = tuple(
        dict.fromkeys(artifact for _, artifacts in generated for artifact in artifacts)
    )
    bundle_digest = suite_artifact.conformance_bundle_digest(
        manifest_bytes,
        suite_document_bytes,
    )
    signoff = spec_signoff_bytes(bundle_digest)
    binding = compile_codec.parse_compile_evidence_binding(
        {
            "kind": "eglur_compile_evidence_binding",
            "schema_version": "0.1",
            "specification": {
                "release": inventory.release,
                "commit": SPECIFICATION_COMMIT,
                "spec_package_signoff_digest": (
                    artifact_content.raw_artifact_digest(signoff)
                ),
                "conformance_bundle_digest": bundle_digest,
            },
            "run_id": RUN_ID,
            "context_digest": context_digest,
            "observation_digests": sorted(
                compile_codec.compile_observation_digest(observation)
                for observation in observations
            ),
        }
    )
    return EvidenceBundle(
        manifest_bytes=manifest_bytes,
        suite_document_bytes=suite_document_bytes,
        context=context,
        observations=observations,
        raw_artifacts=raw_artifacts,
        signoff_bytes=signoff,
        binding=binding,
    )


def raw_artifacts_for_observations(
    bundle: EvidenceBundle,
    observations: tuple[compile_model.CompileObservation, ...],
    *additional_artifacts: bytes,
) -> tuple[bytes, ...]:
    """Select the exact deduplicated raw byte set referenced by observations."""
    available = {
        artifact_content.raw_artifact_digest(content): content
        for content in (*bundle.raw_artifacts, *additional_artifacts)
    }
    required = {
        digest
        for observation in observations
        for digest in (
            observation.compiler_result_digest,
            observation.log_digest,
        )
    }
    return tuple(available[digest] for digest in sorted(required))


def validate_bundle(
    bundle: EvidenceBundle,
) -> list[ValidationProblem]:
    """Validate one complete test bundle."""
    return compile_policy.validate_compile_evidence(
        (bundle.context,),
        bundle.observations,
        bundle.raw_artifacts,
        bundle.binding,
        spec_package_signoff_bytes=bundle.signoff_bytes,
        manifest_bytes=bundle.manifest_bytes,
        suite_document_bytes=bundle.suite_document_bytes,
    )


def validate_malformed_compile_evidence(
    *,
    contexts: object,
    observations: object,
    raw_artifacts: object,
    binding: object,
    spec_package_signoff_bytes: object,
    manifest_bytes: object,
    suite_document_bytes: object,
) -> list[ValidationProblem]:
    """Exercise defensive ingress checks with intentionally invalid runtime values."""
    return compile_policy.validate_compile_evidence(
        cast(tuple[context_model.ExecutionContext, ...], contexts),
        cast(tuple[compile_model.CompileObservation, ...], observations),
        cast(tuple[bytes, ...], raw_artifacts),
        cast(compile_model.CompileEvidenceBinding, binding),
        spec_package_signoff_bytes=cast(bytes, spec_package_signoff_bytes),
        manifest_bytes=cast(bytes, manifest_bytes),
        suite_document_bytes=cast(Mapping[str, bytes], suite_document_bytes),
    )


def validate_malformed_signoff_binding(
    content: object,
    specification: object,
) -> list[ValidationProblem]:
    """Exercise sign-off validation with an invalid boundary value."""
    return compile_policy.validate_spec_package_signoff_binding(
        cast(bytes, content),
        cast(compile_model.SpecificationBinding, specification),
    )


class CompileEvidenceTests(unittest.TestCase):
    """Keep compilation observations phase-specific and fail-closed."""

    def test_synthetic_required_inventory_has_no_problems(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)

        self.assertEqual(validate_bundle(bundle), [])
        self.assertEqual(len(bundle.observations), 3)
        self.assertEqual(
            {item.outcome.kind for item in bundle.observations},
            {"rejected", "succeeded"},
        )

    def test_repository_required_inventory_has_exact_per_fixture_evidence(
        self,
    ) -> None:
        root = TOOLS_DIR.parent
        manifest_bytes = (root / "manifest.yaml").read_bytes()
        manifest = load_yaml_text(manifest_bytes.decode("utf-8"))
        self.assertIsInstance(manifest, dict)
        raw_suites = manifest["suites"]
        self.assertIsInstance(raw_suites, list)
        suite_document_bytes: dict[str, bytes] = {}
        for raw_suite in raw_suites:
            self.assertIsInstance(raw_suite, dict)
            suite_file = raw_suite["file"]
            self.assertIsInstance(suite_file, str)
            content = (root / suite_file).read_bytes()
            suite_document_bytes[suite_file] = content

        inventory = compile_projection.conformance_compile_inventory(
            suite_artifact.parse_conformance_bundle_bytes(
                manifest_bytes,
                suite_document_bytes,
            )
        )
        self.assertEqual(len(inventory.fixtures), 1934)
        self.assertEqual(
            sum(item.expected_diagnostic is None for item in inventory.fixtures),
            970,
        )
        self.assertEqual(
            sum(item.expected_diagnostic is not None for item in inventory.fixtures),
            964,
        )
        invalid_utf8 = next(
            item
            for item in inventory.fixtures
            if item.identity.test_id
            == "LEX-invalid-utf8-source-is-rejected-before-tokenization"
        )
        self.assertIsNotNone(invalid_utf8.expected_diagnostic)
        if invalid_utf8.expected_diagnostic is None:
            self.fail("invalid UTF-8 fixture lost its diagnostic assertion")
        span = invalid_utf8.expected_diagnostic.primary_span
        self.assertIsInstance(span, suite_model.BytePrimarySpan)
        if not isinstance(span, suite_model.BytePrimarySpan):
            self.fail("invalid UTF-8 fixture lost its byte-span assertion")
        self.assertEqual(span.source, "code")
        self.assertEqual(span.byte_range, diagnostic_model.ByteRange(0, 1))
        bundle = build_bundle(
            manifest_bytes,
            suite_document_bytes,
            active_preconditions=(
                "console_write_failure_probe",
                "ffi_linkage_probe",
                "par_traverse_quiescence_probe",
                "scoped_close_failure_probe",
                "strict_unavailable_path",
            ),
        )
        self.assertEqual(validate_bundle(bundle), [])
        self.assertEqual(len(bundle.observations), len(inventory.fixtures))

    def test_compile_outcome_variants_are_closed_and_distinct(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        succeeded = next(
            item for item in bundle.observations if item.outcome.kind == "succeeded"
        )
        rejected = next(
            item for item in bundle.observations if item.outcome.kind == "rejected"
        )

        succeeded_payload = compile_codec.compile_observation_payload(succeeded)
        succeeded_outcome = succeeded_payload["outcome"]
        self.assertIsInstance(succeeded_outcome, dict)
        succeeded_outcome["primary_diagnostic"] = {}
        with self.assertRaisesRegex(ValueError, "unsupported"):
            compile_codec.parse_compile_observation(succeeded_payload)

        rejected_payload = compile_codec.compile_observation_payload(rejected)
        rejected_outcome = rejected_payload["outcome"]
        self.assertIsInstance(rejected_outcome, dict)
        del rejected_outcome["primary_diagnostic"]
        with self.assertRaisesRegex(ValueError, "missing"):
            compile_codec.parse_compile_observation(rejected_payload)

        unknown_payload = compile_codec.compile_observation_payload(succeeded)
        unknown_outcome = unknown_payload["outcome"]
        self.assertIsInstance(unknown_outcome, dict)
        unknown_outcome["kind"] = "passed"
        with self.assertRaisesRegex(ValueError, "must be one of"):
            compile_codec.parse_compile_observation(unknown_payload)

    def test_compiler_result_primary_span_variants_are_closed(self) -> None:
        coordinate = diagnostic_model.SourceCoordinate(2, 4)
        source_spans: tuple[diagnostic_model.PrimarySpan, ...] = (
            diagnostic_model.SourceSpan(
                source="main.eglur",
                byte_range=diagnostic_model.ByteRange(12, 13),
                text_range=diagnostic_model.TextRange(
                    start=coordinate,
                    end=diagnostic_model.SourceCoordinate(2, 5),
                ),
            ),
            diagnostic_model.SourceSpan(
                source="invalid.eglur",
                byte_range=diagnostic_model.ByteRange(0, 1),
            ),
            diagnostic_model.UnavailableSpan(),
        )
        for primary_span in source_spans:
            result = compile_model.CompilerResult(
                compile_model.CompileRejected(
                    compile_model.ObservedDiagnostic(
                        severity="error",
                        code="LEX_INVALID_CHAR",
                        message="lex invalid char",
                        data=(),
                        primary_span=primary_span,
                    )
                )
            )
            payload = compile_codec.compiler_result_payload(result)
            encoded = artifact_content.canonical_json_bytes(payload)
            with self.subTest(primary_span=primary_span):
                self.assertEqual(
                    compile_codec.parse_compiler_result_bytes(encoded),
                    result,
                )

        result = compile_model.CompilerResult(
            compile_model.CompileRejected(
                compile_model.ObservedDiagnostic(
                    severity="error",
                    code="LEX_INVALID_CHAR",
                    message="lex invalid char",
                    data=(),
                    primary_span=source_spans[0],
                )
            )
        )
        baseline = compile_codec.compiler_result_payload(result)

        def primary_span_payload(payload: dict[str, object]) -> dict[str, object]:
            outcome = cast(dict[str, object], payload["outcome"])
            diagnostic = cast(dict[str, object], outcome["primary_diagnostic"])
            return cast(dict[str, object], diagnostic["primary_span"])

        invalid_payloads: list[dict[str, object]] = []
        reversed_bytes = cast(dict[str, object], copy.deepcopy(baseline))
        byte_range = cast(
            dict[str, object],
            primary_span_payload(reversed_bytes)["bytes"],
        )
        byte_range["end"] = 11
        invalid_payloads.append(reversed_bytes)

        nonpositive_text = cast(dict[str, object], copy.deepcopy(baseline))
        text_range = cast(
            dict[str, object],
            primary_span_payload(nonpositive_text)["text"],
        )
        start = cast(dict[str, object], text_range["start"])
        start["line"] = 0
        invalid_payloads.append(nonpositive_text)

        unknown_variant = cast(dict[str, object], copy.deepcopy(baseline))
        primary_span_payload(unknown_variant)["kind"] = "unknown"
        invalid_payloads.append(unknown_variant)

        for payload in invalid_payloads:
            with self.subTest(payload=payload):
                try:
                    compile_codec.parse_compiler_result_bytes(
                        artifact_content.canonical_json_bytes(
                            cast(dict[str, artifact_content.JsonValue], payload)
                        )
                    )
                except (TypeError, ValueError):
                    continue
                self.fail("invalid primary span was accepted")

    def test_compiler_result_is_exactly_one_utf8_json_document(self) -> None:
        result = compile_model.CompilerResult(outcome=compile_model.CompileSucceeded())
        encoded = artifact_content.canonical_json_bytes(
            compile_codec.compiler_result_payload(result)
        )

        self.assertEqual(
            compile_codec.parse_compiler_result_bytes(b" \n" + encoded + b"\n"),
            result,
        )

        malformed = (
            encoded + b"\n{}",
            b"\xef\xbb\xbf" + encoded,
            (
                b'{"kind":"eglur_compile_result",'
                b'"kind":"eglur_compile_result",'
                b'"schema_version":"0.1",'
                b'"outcome":{"kind":"succeeded"}}'
            ),
            (
                b'{"kind":"eglur_compile_result",'
                b'"schema_version":"0.1",'
                b'"outcome":{"kind":"succeeded"},"extra":NaN}'
            ),
            b"\xff",
            b"[" * 2_000 + b"0" + b"]" * 2_000,
        )
        for content in malformed:
            with self.subTest(content=content):
                try:
                    compile_codec.parse_compiler_result_bytes(content)
                except (UnicodeDecodeError, TypeError, ValueError):
                    continue
                self.fail("malformed compiler result was accepted")

        deep = (
            b'{"kind":"eglur_compile_result",'
            b'"schema_version":"0.1",'
            b'"outcome":{"kind":"succeeded"},'
            b'"extra":'
            + b"[" * compile_codec.MAX_COMPILER_RESULT_JSON_DEPTH
            + b"0"
            + b"]" * compile_codec.MAX_COMPILER_RESULT_JSON_DEPTH
            + b"}"
        )
        with self.assertRaisesRegex(ValueError, "nesting depth"):
            compile_codec.parse_compiler_result_bytes(deep)

        with self.assertRaisesRegex(ValueError, "exceeds 1048576 bytes"):
            compile_codec.parse_compiler_result_bytes(
                b" " * (compile_codec.MAX_COMPILER_RESULT_BYTES + 1)
            )

    def test_process_termination_variants_are_closed(self) -> None:
        exited = compile_model.ExitedProcess(code=0)
        signaled = compile_model.SignaledProcess(signal=9)
        for termination in (exited, signaled):
            with self.subTest(termination=termination):
                self.assertEqual(
                    compile_codec.parse_process_termination(
                        compile_codec.process_termination_payload(termination),
                        "termination",
                    ),
                    termination,
                )

        invalid = (
            {"kind": "exited", "code": -1},
            {"kind": "exited", "code": True},
            {"kind": "signaled", "signal": 0},
            {"kind": "timed_out"},
            {"kind": "exited", "code": 0, "signal": 9},
        )
        for termination in invalid:
            with self.subTest(termination=termination):
                try:
                    compile_codec.parse_process_termination(
                        termination,
                        "termination",
                    )
                except (TypeError, ValueError):
                    continue
                self.fail("invalid process termination was accepted")

    def test_tagged_diagnostic_scalars_preserve_kind_and_exact_value(self) -> None:
        artifacts: tuple[tuple[YamlValue, compile_model.DiagnosticScalar], ...] = (
            (
                {"kind": "bool", "value": True},
                compile_model.DiagnosticScalar("bool", True),
            ),
            (
                {"kind": "int", "value": 1},
                compile_model.DiagnosticScalar("int", 1),
            ),
            (
                {"kind": "str", "value": ""},
                compile_model.DiagnosticScalar("str", ""),
            ),
        )
        for artifact, expected in artifacts:
            with self.subTest(artifact=artifact):
                self.assertEqual(
                    compile_codec.parse_diagnostic_scalar(artifact, "scalar"),
                    expected,
                )

        invalid = (
            {"kind": "int", "value": True},
            {"kind": "int", "value": -(2**63) - 1},
            {"kind": "int", "value": 2**63},
            {"kind": "float", "value": "1.0"},
            {"kind": "str", "value": "\ud800"},
            {"kind": "null"},
        )
        for artifact in invalid:
            with self.subTest(artifact=artifact):
                try:
                    compile_codec.parse_diagnostic_scalar(artifact, "scalar")
                except (TypeError, ValueError):
                    continue
                self.fail("invalid diagnostic scalar was accepted")

    def test_compilation_input_digest_covers_every_invocation_field(self) -> None:
        def digest(test_id: str, fixture: dict[str, YamlValue]) -> str:
            return compile_projection.compilation_input_digest(
                suite_model.TestIdentity("suite", test_id),
                suite_codec.decode_fixture_invocation(fixture),
            )

        base: dict[str, YamlValue] = {
            "code": "fn main() -> Unit = ()\n",
        }
        original = digest("test", base)
        mutations: tuple[dict[str, YamlValue], ...] = (
            {"code": "fn main() -> Unit = ( )\n"},
            {**base, "mode": "file-set"},
            {**base, "runner": "test"},
        )
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.assertNotEqual(
                    digest("test", mutation),
                    original,
                )

        files: dict[str, YamlValue] = {
            "files": {
                "app/main.eglur": "fn main() -> Unit = ()\n",
                "app/lib.eglur": "fn f() -> Unit = ()\n",
            },
            "symlinks": {"app/alias": "lib.eglur"},
            "entry": "app/main.eglur",
            "file_set_root": ".",
            "mode": "file-set",
        }
        files_digest = digest("files", files)
        for field, value in (
            ("entry", "app/lib.eglur"),
            ("file_set_root", "app"),
        ):
            mutation = {**files, field: value}
            with self.subTest(field=field):
                self.assertNotEqual(
                    digest("files", mutation),
                    files_digest,
                )

        for symlinks in (
            {"app/alias-two": "lib.eglur"},
            {"app/alias": "./lib.eglur"},
        ):
            with self.subTest(symlinks=symlinks):
                self.assertNotEqual(
                    digest("files", {**files, "symlinks": symlinks}),
                    files_digest,
                )

        registry_fixture: dict[str, YamlValue] = {
            "files": {
                "Eglur.toml": '[package]\nname = "app"\nversion = "0.1.0"\n',
                "src/main.eglur": "fn main!() -> Unit = ()\n",
            },
            "entry": "src/main.eglur",
            "mode": "package",
            "registry_acquisitions": [
                {
                    "package": "packages.example.com/acme/math",
                    "version": "1.2.3",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path": "src/dup.eglur",
                                "text": "z",
                            }
                        },
                        {
                            "emit": {
                                "kind": "file",
                                "path": "src/dup.eglur",
                                "content_hex": "00",
                            }
                        },
                        {"emit": {"kind": "directory", "path": "src"}},
                        {
                            "emit": {
                                "kind": "symlink",
                                "path": "src/alias",
                                "target": "dup.eglur",
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ],
        }
        registry_digest = digest("registry", registry_fixture)
        reordered_registry_fixture: dict[str, YamlValue] = {
            **registry_fixture,
            "registry_acquisitions": [
                {
                    "version": "1.2.3",
                    "package": "packages.example.com/acme/math",
                    "provider_steps": [
                        {
                            "emit": {
                                "target": "dup.eglur",
                                "path": "src/alias",
                                "kind": "symlink",
                            }
                        },
                        {"emit": {"path": "src", "kind": "directory"}},
                        {
                            "emit": {
                                "content_hex": "00",
                                "path": "src/dup.eglur",
                                "kind": "file",
                            }
                        },
                        {
                            "emit": {
                                "text": "z",
                                "path": "src/dup.eglur",
                                "kind": "file",
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ],
        }
        self.assertNotEqual(
            digest("registry", reordered_registry_fixture),
            registry_digest,
        )
        hex_spelled_registry_fixture: dict[str, YamlValue] = {
            **registry_fixture,
            "registry_acquisitions": [
                {
                    "package": "packages.example.com/acme/math",
                    "version": "1.2.3",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path_hex": "7372632f6475702e65676c7572",
                                "content_hex": "7a",
                            }
                        },
                        {
                            "emit": {
                                "kind": "file",
                                "path_hex": "7372632f6475702e65676c7572",
                                "content_hex": "00",
                            }
                        },
                        {
                            "emit": {
                                "kind": "directory",
                                "path_hex": "737263",
                            }
                        },
                        {
                            "emit": {
                                "kind": "symlink",
                                "path_hex": "7372632f616c696173",
                                "target_hex": "6475702e65676c7572",
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ],
        }
        self.assertEqual(
            digest("registry", hex_spelled_registry_fixture),
            registry_digest,
        )
        changed_registry_fixture: dict[str, YamlValue] = {
            **registry_fixture,
            "registry_acquisitions": [
                {
                    "package": "packages.example.com/acme/math",
                    "version": "1.2.3",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path": "src/dup.eglur",
                                "text": "changed",
                            }
                        },
                        {
                            "emit": {
                                "kind": "file",
                                "path": "src/dup.eglur",
                                "content_hex": "00",
                            }
                        },
                        {"emit": {"kind": "directory", "path": "src"}},
                        {
                            "emit": {
                                "kind": "symlink",
                                "path": "src/alias",
                                "target": "dup.eglur",
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ],
        }
        self.assertNotEqual(
            digest("registry", changed_registry_fixture),
            registry_digest,
        )

        registry_payload = compile_projection.compilation_input_payload(
            suite_model.TestIdentity("suite", "registry"),
            suite_codec.decode_fixture_invocation(registry_fixture),
        )
        self.assertEqual(
            registry_payload["registry_acquisitions"],
            [
                {
                    "package": "packages.example.com/acme/math",
                    "version": "1.2.3",
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path_hex": "7372632f6475702e65676c7572",
                                "content_hex": "7a",
                            }
                        },
                        {
                            "emit": {
                                "kind": "file",
                                "path_hex": "7372632f6475702e65676c7572",
                                "content_hex": "00",
                            }
                        },
                        {
                            "emit": {
                                "kind": "directory",
                                "path_hex": "737263",
                            }
                        },
                        {
                            "emit": {
                                "kind": "symlink",
                                "path_hex": "7372632f616c696173",
                                "target_hex": "6475702e65676c7572",
                            }
                        },
                        {"finish": "complete"},
                    ],
                }
            ],
        )

        source_payload = compile_projection.compilation_input_payload(
            suite_model.TestIdentity("suite", "files-without-topology"),
            suite_codec.decode_fixture_invocation(
                {
                    "files": {"main.eglur": "fn main() -> Unit = ()\n"},
                    "entry": "main.eglur",
                }
            ),
        )
        self.assertEqual(
            source_payload,
            {
                "suite_id": "suite",
                "test_id": "files-without-topology",
                "mode": "auto",
                "runner": "main",
                "registry_acquisitions": [],
                "source": {
                    "kind": "files",
                    "files": {"main.eglur": "fn main() -> Unit = ()\n"},
                    "symlinks": {},
                    "entry": "main.eglur",
                    "file_set_root": None,
                },
            },
        )

        self.assertEqual(
            digest("bytes", {"code_bytes_hex": "FF"}),
            digest("bytes", {"code_bytes_hex": "ff"}),
        )

    def test_compilation_input_payload_preserves_registry_provider_scripts(
        self,
    ) -> None:
        fixture: dict[str, YamlValue] = {
            "files": {
                "Eglur.toml": '[package]\nname = "app"\nversion = "0.1.0"\n',
                "src/main.eglur": "fn main!() -> Unit = ()\n",
            },
            "entry": "src/main.eglur",
            "mode": "package",
        }
        cases: tuple[tuple[dict[str, YamlValue], dict[str, YamlValue]], ...] = (
            (
                {"provider_steps": [{"finish": "missing_release"}]},
                {"provider_steps": [{"finish": "missing_release"}]},
            ),
            (
                {
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path": "partial",
                                "text": "data",
                            }
                        },
                        {"finish": "transport_failure"},
                    ]
                },
                {
                    "provider_steps": [
                        {
                            "emit": {
                                "kind": "file",
                                "path_hex": "7061727469616c",
                                "content_hex": "64617461",
                            }
                        },
                        {"finish": "transport_failure"},
                    ]
                },
            ),
            (
                {
                    "provider_steps": [
                        {
                            "repeat": {
                                "count": 65_537,
                                "emit": {
                                    "kind": "directory",
                                    "path_repeat": {
                                        "byte_hex": "61",
                                        "count": 4_096,
                                    },
                                },
                            }
                        },
                        {"finish": "complete"},
                    ]
                },
                {
                    "provider_steps": [
                        {
                            "repeat": {
                                "count": 65_537,
                                "emit": {
                                    "kind": "directory",
                                    "path_repeat": {
                                        "byte_hex": "61",
                                        "count": 4_096,
                                    },
                                },
                            }
                        },
                        {"finish": "complete"},
                    ]
                },
            ),
        )
        for acquisition_fields, expected_fields in cases:
            acquisition: dict[str, YamlValue] = {
                "package": "packages.example.com/acme/math",
                "version": "1.2.3",
                **acquisition_fields,
            }
            invocation = suite_codec.decode_fixture_invocation(
                {**fixture, "registry_acquisitions": [acquisition]}
            )
            payload = compile_projection.compilation_input_payload(
                suite_model.TestIdentity("suite", "registry-failure"),
                invocation,
            )

            self.assertEqual(
                payload["registry_acquisitions"],
                [
                    {
                        "package": "packages.example.com/acme/math",
                        "version": "1.2.3",
                        **expected_fields,
                    }
                ],
            )

    def test_missing_record_fails_even_when_binding_and_raw_set_are_consistent(
        self,
    ) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        observations = bundle.observations[:-1]
        raw_artifacts = raw_artifacts_for_observations(bundle, observations)
        partial = replace(
            bundle,
            observations=observations,
            raw_artifacts=raw_artifacts,
            binding=rebind(bundle.binding, observations),
        )

        messages = [problem.message for problem in validate_bundle(partial)]
        self.assertTrue(
            any("required compile observation is missing" in item for item in messages)
        )

    def test_bundle_bytes_must_match_the_binding_address(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        tampered = replace(
            bundle,
            manifest_bytes=bundle.manifest_bytes + b"\n# tampered\n",
        )

        messages = [problem.message for problem in validate_bundle(tampered)]
        self.assertTrue(
            any("do not match the bound bundle digest" in item for item in messages)
        )

    def test_conformance_bundle_digest_matches_the_golden_vector(self) -> None:
        digest = suite_artifact.conformance_bundle_digest(
            b"manifest\n",
            {
                "b.yaml": b"B\n",
                "a.yaml": b"A\n",
            },
        )

        self.assertEqual(
            digest,
            "sha256:30b69bdae8fdc4b6ddbe124061f1f5571032c4d483ffc21d114efb6741494d81",
        )

    def test_signed_digest_blocks_coherent_inventory_rebinding(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        suite_path = "core/synthetic.yaml"
        suite = load_yaml_text(bundle.suite_document_bytes[suite_path].decode("utf-8"))
        self.assertIsInstance(suite, dict)
        tests = suite["tests"]
        self.assertIsInstance(tests, list)
        suite["tests"] = [
            test
            for test in tests
            if isinstance(test, dict) and test.get("id") != "SYN-positive"
        ]
        shrunk_suite_bytes = {
            **bundle.suite_document_bytes,
            suite_path: yaml_bytes(suite),
        }
        observations = tuple(
            observation
            for observation in bundle.observations
            if observation.test_id != "SYN-positive"
        )
        required_raw_digests = {
            digest
            for observation in observations
            for digest in (
                observation.compiler_result_digest,
                observation.log_digest,
            )
        }
        raw_artifacts = tuple(
            content
            for content in bundle.raw_artifacts
            if artifact_content.raw_artifact_digest(content) in required_raw_digests
        )
        shrunk_digest = suite_artifact.conformance_bundle_digest(
            bundle.manifest_bytes,
            shrunk_suite_bytes,
        )
        binding = rebind(bundle.binding, observations)
        binding = replace(
            binding,
            specification=replace(
                binding.specification,
                conformance_bundle_digest=shrunk_digest,
            ),
        )
        shrunk = replace(
            bundle,
            suite_document_bytes=shrunk_suite_bytes,
            observations=observations,
            raw_artifacts=raw_artifacts,
            binding=binding,
        )

        messages = [problem.message for problem in validate_bundle(shrunk)]
        self.assertTrue(
            any("sign-off conformance bundle digest is" in item for item in messages)
        )
        self.assertFalse(
            any(
                token in item
                for token in (
                    "required compile observation is missing",
                    "do not match the bound bundle digest",
                )
                for item in messages
            )
        )

    def test_bundle_bytes_are_snapshotted_before_hash_and_parse(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        malformed_yaml = b"not: [yaml"
        split_manifest = SplitViewBytes(
            malformed_yaml,
            bundle.manifest_bytes.decode("utf-8"),
        )
        suite_path = min(bundle.suite_document_bytes)
        split_suite = SplitViewBytes(
            malformed_yaml,
            bundle.suite_document_bytes[suite_path].decode("utf-8"),
        )
        mutations = (
            (
                split_manifest,
                bundle.suite_document_bytes,
            ),
            (
                bundle.manifest_bytes,
                {
                    **bundle.suite_document_bytes,
                    suite_path: split_suite,
                },
            ),
        )
        for manifest_bytes, suite_document_bytes in mutations:
            bundle_digest = suite_artifact.conformance_bundle_digest(
                manifest_bytes,
                suite_document_bytes,
            )
            signoff = spec_signoff_bytes(bundle_digest)
            binding = replace(
                bundle.binding,
                specification=replace(
                    bundle.binding.specification,
                    spec_package_signoff_digest=(
                        artifact_content.raw_artifact_digest(signoff)
                    ),
                    conformance_bundle_digest=bundle_digest,
                ),
            )
            mutated = replace(
                bundle,
                manifest_bytes=manifest_bytes,
                suite_document_bytes=suite_document_bytes,
                signoff_bytes=signoff,
                binding=binding,
            )

            with self.subTest(manifest_bytes=manifest_bytes):
                messages = [problem.message for problem in validate_bundle(mutated)]
                self.assertTrue(
                    any(
                        "invalid compile fixture inventory" in item for item in messages
                    )
                )

    def test_signoff_bytes_are_snapshotted_before_hash_and_parse(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        split_signoff = SplitViewBytes(
            b"not: [yaml",
            bundle.signoff_bytes.decode("utf-8"),
        )
        binding = replace(
            bundle.binding,
            specification=replace(
                bundle.binding.specification,
                spec_package_signoff_digest=(
                    artifact_content.raw_artifact_digest(split_signoff)
                ),
            ),
        )
        mutated = replace(
            bundle,
            signoff_bytes=split_signoff,
            binding=binding,
        )

        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertTrue(
            any("invalid spec-package sign-off artifact" in item for item in messages)
        )

    def test_stateful_suite_mapping_is_snapshotted_once(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        suite_path = "core/synthetic.yaml"
        suite = load_yaml_text(bundle.suite_document_bytes[suite_path].decode("utf-8"))
        self.assertIsInstance(suite, dict)
        tests = suite["tests"]
        self.assertIsInstance(tests, list)
        suite["tests"] = [
            test
            for test in tests
            if isinstance(test, dict) and test.get("id") != "SYN-positive"
        ]
        shrunk_suite_bytes = {
            **bundle.suite_document_bytes,
            suite_path: yaml_bytes(suite),
        }
        observations = tuple(
            observation
            for observation in bundle.observations
            if observation.test_id != "SYN-positive"
        )
        required_raw_digests = {
            digest
            for observation in observations
            for digest in (
                observation.compiler_result_digest,
                observation.log_digest,
            )
        }
        raw_artifacts = tuple(
            content
            for content in bundle.raw_artifacts
            if artifact_content.raw_artifact_digest(content) in required_raw_digests
        )
        switching = SwitchingSuiteDocuments(
            bundle.suite_document_bytes,
            shrunk_suite_bytes,
        )
        mutated = replace(
            bundle,
            suite_document_bytes=switching,
            observations=observations,
            raw_artifacts=raw_artifacts,
            binding=rebind(bundle.binding, observations),
        )

        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertEqual(switching.items_calls, 1)
        self.assertTrue(
            any("required compile observation is missing" in item for item in messages)
        )

    def test_extra_and_duplicate_fixture_observations_fail(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        extra = replace(
            bundle.observations[0],
            suite_id="optional-synthetic",
            test_id="SYN-optional",
        )
        observations = (*bundle.observations, extra)
        extra_bundle = replace(
            bundle,
            observations=observations,
            binding=rebind(bundle.binding, observations),
        )
        messages = [problem.message for problem in validate_bundle(extra_bundle)]
        self.assertTrue(
            any("not an active required fixture" in item for item in messages)
        )

        duplicate = replace(
            bundle.observations[0],
            compiler_result_digest=bundle.observations[1].compiler_result_digest,
        )
        observations = (*bundle.observations, duplicate)
        duplicate_bundle = replace(
            bundle,
            observations=observations,
            binding=rebind(bundle.binding, observations),
        )
        messages = [problem.message for problem in validate_bundle(duplicate_bundle)]
        self.assertTrue(
            any("duplicate compile observation identity" in item for item in messages)
        )

    def test_mixed_run_context_and_specification_commit_fail(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        mutations = (
            replace(bundle.observations[0], run_id="run-other"),
            replace(
                bundle.observations[0],
                context_digest=f"sha256:{'f' * 64}",
            ),
            replace(
                bundle.observations[0],
                specification_commit="f" * 40,
            ),
        )
        expected_tokens = ("run_id", "context digest", "specification commit")
        for mutation, expected_token in zip(
            mutations,
            expected_tokens,
            strict=True,
        ):
            observations = (mutation, *bundle.observations[1:])
            mutated_bundle = replace(
                bundle,
                observations=observations,
                binding=rebind(bundle.binding, observations),
            )
            with self.subTest(expected_token=expected_token):
                messages = [
                    problem.message for problem in validate_bundle(mutated_bundle)
                ]
                self.assertTrue(any(expected_token in item for item in messages))

    def test_binding_and_raw_artifact_sets_are_exact(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)

        stale_binding = replace(
            bundle.binding,
            observation_digests=bundle.binding.observation_digests[:-1],
        )
        messages = [
            problem.message
            for problem in validate_bundle(replace(bundle, binding=stale_binding))
        ]
        self.assertTrue(
            any("compile observation is not bound" in item for item in messages)
        )

        messages = [
            problem.message
            for problem in validate_bundle(
                replace(bundle, raw_artifacts=bundle.raw_artifacts[:-1])
            )
        ]
        self.assertTrue(
            any("raw compile artifact is missing" in item for item in messages)
        )

        messages = [
            problem.message
            for problem in validate_bundle(
                replace(
                    bundle,
                    raw_artifacts=(*bundle.raw_artifacts, b"unbound"),
                )
            )
        ]
        self.assertTrue(
            any("raw compile artifact is not bound" in item for item in messages)
        )

        messages = [
            problem.message
            for problem in validate_bundle(
                replace(
                    bundle,
                    raw_artifacts=(
                        *bundle.raw_artifacts,
                        bundle.raw_artifacts[0],
                    ),
                )
            )
        ]
        self.assertTrue(
            any("duplicate raw compile artifact" in item for item in messages)
        )

    def test_raw_stdout_result_and_observation_must_agree(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        positive = next(
            item for item in bundle.observations if item.test_id == "SYN-positive"
        )
        negative = next(
            item for item in bundle.observations if item.test_id == "SYN-negative"
        )
        conflicting_stdout = (
            artifact_content.canonical_json_bytes(
                compile_codec.compiler_result_payload(
                    compile_model.CompilerResult(outcome=negative.outcome)
                )
            )
            + b"\n"
        )
        conflicting_digest = artifact_content.raw_artifact_digest(conflicting_stdout)
        changed_positive = replace(
            positive,
            compiler_result_digest=conflicting_digest,
        )
        observations = tuple(
            changed_positive if item is positive else item
            for item in bundle.observations
        )
        raw_artifacts = raw_artifacts_for_observations(
            bundle,
            observations,
            conflicting_stdout,
        )
        mutated = replace(
            bundle,
            observations=observations,
            raw_artifacts=raw_artifacts,
            binding=rebind(bundle.binding, observations),
        )

        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertTrue(
            any("compiler result outcome does not match" in item for item in messages)
        )

    def test_malformed_compiler_stdout_fails_closed(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        original = bundle.observations[0]
        malformed_stdout = b"{}\n{}\n"
        malformed_digest = artifact_content.raw_artifact_digest(malformed_stdout)
        changed = replace(
            original,
            compiler_result_digest=malformed_digest,
        )
        observations = (changed, *bundle.observations[1:])
        raw_artifacts = raw_artifacts_for_observations(
            bundle,
            observations,
            malformed_stdout,
        )
        mutated = replace(
            bundle,
            observations=observations,
            raw_artifacts=raw_artifacts,
            binding=rebind(bundle.binding, observations),
        )

        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertTrue(
            any("invalid compiler result stdout artifact" in item for item in messages)
        )

    def test_raw_digest_and_json_parser_share_one_byte_snapshot(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        positive = next(
            item for item in bundle.observations if item.test_id == "SYN-positive"
        )
        valid_stdout = artifact_content.canonical_json_bytes(
            compile_codec.compiler_result_payload(
                compile_model.CompilerResult(outcome=positive.outcome)
            )
        )
        split_view = SplitViewBytes(
            b"not-json",
            valid_stdout.decode("utf-8"),
        )
        split_digest = artifact_content.raw_artifact_digest(split_view)
        changed = replace(
            positive,
            compiler_result_digest=split_digest,
        )
        observations = tuple(
            changed if item is positive else item for item in bundle.observations
        )
        raw_artifacts = raw_artifacts_for_observations(
            bundle,
            observations,
            split_view,
        )
        mutated = replace(
            bundle,
            observations=observations,
            raw_artifacts=raw_artifacts,
            binding=rebind(bundle.binding, observations),
        )

        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertTrue(
            any("invalid compiler result stdout artifact" in item for item in messages)
        )

    def test_process_termination_must_match_compile_outcome(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        positive = next(
            item for item in bundle.observations if item.test_id == "SYN-positive"
        )
        invalid_terminations = (
            (
                compile_model.ExitedProcess(code=1),
                "exit code is 1, expected 0",
            ),
            (
                compile_model.SignaledProcess(signal=9),
                "terminated by signal 9",
            ),
        )
        for termination, expected in invalid_terminations:
            changed = replace(positive, termination=termination)
            observations = tuple(
                changed if item is positive else item for item in bundle.observations
            )
            mutated = replace(
                bundle,
                observations=observations,
                binding=rebind(bundle.binding, observations),
            )
            with self.subTest(termination=termination):
                messages = [problem.message for problem in validate_bundle(mutated)]
                self.assertTrue(any(expected in item for item in messages))

    def test_positive_rejection_and_negative_success_are_not_whole_test_passes(
        self,
    ) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        positive = next(
            item for item in bundle.observations if item.test_id == "SYN-positive"
        )
        negative = next(
            item for item in bundle.observations if item.test_id == "SYN-negative"
        )
        rejected_positive = replace(
            positive,
            outcome=negative.outcome,
        )
        succeeded_negative = replace(
            negative,
            outcome=positive.outcome,
        )
        observations = tuple(
            rejected_positive
            if item is positive
            else succeeded_negative
            if item is negative
            else item
            for item in bundle.observations
        )
        mutated = replace(
            bundle,
            observations=observations,
            binding=rebind(bundle.binding, observations),
        )

        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertTrue(
            any("positive fixture was rejected" in item for item in messages)
        )
        self.assertTrue(
            any("negative fixture was succeeded" in item for item in messages)
        )

    def test_negative_diagnostic_contract_is_compared_field_by_field(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        negative = next(
            item for item in bundle.observations if item.test_id == "SYN-negative"
        )
        self.assertIsInstance(negative.outcome, compile_model.CompileRejected)
        if not isinstance(negative.outcome, compile_model.CompileRejected):
            self.fail("negative fixture outcome was not narrowed")
        rejected_outcome = negative.outcome
        diagnostic = rejected_outcome.primary_diagnostic
        self.assertIsInstance(diagnostic.primary_span, diagnostic_model.SourceSpan)
        if not isinstance(diagnostic.primary_span, diagnostic_model.SourceSpan):
            self.fail("negative fixture span was not narrowed")
        primary_span = diagnostic.primary_span
        self.assertIsNotNone(primary_span.text_range)
        if primary_span.text_range is None:
            self.fail("negative fixture text range was not narrowed")
        data = dict(diagnostic.data)
        mutations = (
            (
                replace(diagnostic, code="OTHER_ERROR"),
                "diagnostic code",
            ),
            (
                replace(diagnostic, message="missing"),
                "message is missing",
            ),
            (
                replace(diagnostic, message="name"),
                "matches none",
            ),
            (
                replace(diagnostic, data=()),
                "data is missing keys",
            ),
            (
                replace(
                    diagnostic,
                    data=(
                        (
                            "name",
                            compile_model.DiagnosticScalar(
                                kind="str",
                                value="other",
                            ),
                        ),
                    ),
                ),
                "data.name",
            ),
            (
                replace(
                    diagnostic,
                    primary_span=replace(
                        primary_span,
                        text_range=diagnostic_model.TextRange(
                            start=diagnostic_model.SourceCoordinate(99, 1),
                            end=diagnostic_model.SourceCoordinate(99, 1),
                        ),
                    ),
                ),
                "diagnostic location",
            ),
        )
        self.assertEqual(
            data["name"],
            compile_model.DiagnosticScalar(kind="str", value="missing"),
        )
        for mutated_diagnostic, expected_token in mutations:
            mutated_observation = replace(
                negative,
                outcome=replace(
                    rejected_outcome,
                    primary_diagnostic=mutated_diagnostic,
                ),
            )
            observations = tuple(
                mutated_observation if item is negative else item
                for item in bundle.observations
            )
            mutated_bundle = replace(
                bundle,
                observations=observations,
                binding=rebind(bundle.binding, observations),
            )
            with self.subTest(expected_token=expected_token):
                messages = [
                    problem.message for problem in validate_bundle(mutated_bundle)
                ]
                self.assertTrue(any(expected_token in item for item in messages))

    def test_input_digest_mismatch_fails_without_source_semantic_inference(
        self,
    ) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        positive = next(
            item for item in bundle.observations if item.test_id == "SYN-positive"
        )
        self.assertEqual(positive.outcome.kind, "succeeded")
        mutated_observation = replace(
            positive,
            input_digest=f"sha256:{'f' * 64}",
        )
        observations = tuple(
            mutated_observation if item is positive else item
            for item in bundle.observations
        )
        mutated = replace(
            bundle,
            observations=observations,
            binding=rebind(bundle.binding, observations),
        )

        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertTrue(any("input digest is" in item for item in messages))

    def test_spec_signoff_bytes_release_and_commit_are_bound(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        tampered = replace(
            bundle,
            signoff_bytes=bundle.signoff_bytes + b"# tampered\n",
        )
        messages = [problem.message for problem in validate_bundle(tampered)]
        self.assertTrue(
            any("do not match the bound digest" in item for item in messages)
        )

        wrong_release = spec_signoff_bytes(
            bundle.binding.specification.conformance_bundle_digest,
            release="v0.1.1",
        )
        binding = replace(
            bundle.binding,
            specification=replace(
                bundle.binding.specification,
                spec_package_signoff_digest=(
                    artifact_content.raw_artifact_digest(wrong_release)
                ),
            ),
        )
        messages = [
            problem.message
            for problem in validate_bundle(
                replace(
                    bundle,
                    signoff_bytes=wrong_release,
                    binding=binding,
                )
            )
        ]
        self.assertTrue(any("sign-off release" in item for item in messages))

        wrong_commit = spec_signoff_bytes(
            bundle.binding.specification.conformance_bundle_digest,
            commit="f" * 40,
        )
        binding = replace(
            bundle.binding,
            specification=replace(
                bundle.binding.specification,
                spec_package_signoff_digest=(
                    artifact_content.raw_artifact_digest(wrong_commit)
                ),
            ),
        )
        messages = [
            problem.message
            for problem in validate_bundle(
                replace(
                    bundle,
                    signoff_bytes=wrong_commit,
                    binding=binding,
                )
            )
        ]
        self.assertTrue(any("sign-off commit" in item for item in messages))

    def test_inactive_required_precondition_excludes_only_its_suite(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        inactive = build_bundle(
            manifest,
            suites,
            active_preconditions=(),
        )
        self.assertEqual(validate_bundle(inactive), [])
        self.assertEqual(
            {item.test_id for item in inactive.observations},
            {"SYN-positive", "SYN-negative"},
        )

        active = build_bundle(manifest, suites)
        conditional = next(
            item
            for item in active.observations
            if item.test_id == "SYN-conditional-positive"
        )
        observations = (*inactive.observations, conditional)
        raw_artifacts = raw_artifacts_for_observations(
            replace(
                inactive,
                raw_artifacts=tuple(
                    dict.fromkeys((*inactive.raw_artifacts, *active.raw_artifacts))
                ),
            ),
            observations,
        )
        mutated = replace(
            inactive,
            observations=observations,
            raw_artifacts=raw_artifacts,
            binding=rebind(inactive.binding, observations),
        )
        messages = [problem.message for problem in validate_bundle(mutated)]
        self.assertTrue(
            any("not an active required fixture" in item for item in messages)
        )

    def test_direct_models_cannot_hide_invalid_variant_state(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        succeeded = next(
            item for item in bundle.observations if item.outcome.kind == "succeeded"
        )
        rejected = next(
            item for item in bundle.observations if item.outcome.kind == "rejected"
        )
        unchecked_replace = cast(Callable[..., object], replace)
        with self.assertRaises(TypeError):
            unchecked_replace(
                succeeded.outcome,
                primary_diagnostic=compile_model.ObservedDiagnostic(
                    severity="error",
                    code="OTHER_ERROR",
                    message="hidden",
                    data=(),
                    primary_span=diagnostic_model.UnavailableSpan(),
                ),
            )
        with self.assertRaises(TypeError):
            unchecked_replace(
                rejected.outcome,
                primary_diagnostic=None,
            )

        missing_outcome = replace(succeeded, outcome=None)
        observations = tuple(
            missing_outcome if item.test_id == missing_outcome.test_id else item
            for item in bundle.observations
        )
        messages = [
            problem.message
            for problem in validate_bundle(replace(bundle, observations=observations))
        ]
        self.assertTrue(any("invalid compile observation" in item for item in messages))

        missing_specification = replace(bundle.binding, specification=None)
        messages = [
            problem.message
            for problem in validate_bundle(
                replace(bundle, binding=missing_specification)
            )
        ]
        self.assertTrue(
            any("invalid compile evidence binding" in item for item in messages)
        )

        messages = [
            problem.message
            for problem in validate_bundle(replace(bundle, context=None))
        ]
        self.assertTrue(
            any("invalid execution context artifact" in item for item in messages)
        )

    def test_malformed_ingress_containers_return_validation_problems(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        inputs: dict[str, object] = {
            "contexts": (bundle.context,),
            "observations": bundle.observations,
            "raw_artifacts": bundle.raw_artifacts,
            "binding": bundle.binding,
            "spec_package_signoff_bytes": bundle.signoff_bytes,
            "manifest_bytes": bundle.manifest_bytes,
            "suite_document_bytes": bundle.suite_document_bytes,
        }
        for field in (
            "contexts",
            "observations",
            "raw_artifacts",
            "spec_package_signoff_bytes",
            "manifest_bytes",
            "suite_document_bytes",
        ):
            malformed = {**inputs, field: None}
            with self.subTest(field=field):
                problems = validate_malformed_compile_evidence(**malformed)
                self.assertTrue(
                    any(f"{field} must be" in problem.message for problem in problems)
                )

        malformed = {**inputs, "raw_artifacts": (None,)}
        problems = validate_malformed_compile_evidence(**malformed)
        self.assertTrue(
            any(
                "invalid raw compile artifact[0]" in problem.message
                for problem in problems
            )
        )

        problems = validate_malformed_signoff_binding(
            None,
            bundle.binding.specification,
        )
        self.assertTrue(
            any(
                "invalid spec-package sign-off artifact" in problem.message
                for problem in problems
            )
        )
        problems = validate_malformed_signoff_binding(
            bundle.signoff_bytes,
            None,
        )
        self.assertTrue(
            any(
                "invalid spec-package sign-off artifact" in problem.message
                for problem in problems
            )
        )

    def test_summary_only_or_empty_observation_bindings_are_rejected(self) -> None:
        manifest, suites = synthetic_inventory_artifacts()
        bundle = build_bundle(manifest, suites)
        payload = compile_codec.compile_evidence_binding_payload(bundle.binding)
        payload["summary"] = {"passed": len(bundle.observations)}
        with self.assertRaisesRegex(ValueError, "unsupported"):
            compile_codec.parse_compile_evidence_binding(payload)

        payload = compile_codec.compile_evidence_binding_payload(bundle.binding)
        payload["observation_digests"] = []
        with self.assertRaisesRegex(ValueError, "must not be empty"):
            compile_codec.parse_compile_evidence_binding(payload)


if __name__ == "__main__":
    unittest.main()

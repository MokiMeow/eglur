"""Strict codecs for semantic fixture selections and generated corpora."""

import json
from typing import cast

from eglur_conformance.artifact_content import (
    JsonValue,
    decode_json_bytes,
    expect_concrete_string,
    expect_exact_keys,
    expect_list,
    expect_mapping,
    expect_non_negative_int,
    expect_positive_int,
    expect_text,
)
from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.semantic_slice import model
from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import model as suite_model
from eglur_conformance.yaml_document import YamlValue, load_yaml_text


def _required_text(value: YamlValue, context: str) -> str:
    return expect_text(value, context, allow_empty=False)


def _string_tuple(value: YamlValue, context: str) -> tuple[str, ...]:
    return tuple(
        expect_text(item, f"{context}[{index}]", allow_empty=True)
        for index, item in enumerate(expect_list(value, context))
    )


def _optional_string_tuple(
    value: YamlValue,
    context: str,
) -> tuple[str, ...] | None:
    return None if value is None else _string_tuple(value, context)


def _decode_selection_identity(
    value: YamlValue, context: str
) -> suite_model.TestIdentity:
    rendered = expect_concrete_string(value, context)
    if rendered.count("/") != 1:
        raise ValueError(f"{context} must use canonical suite/test identity syntax")
    suite_id, test_id = rendered.split("/", 1)
    return suite_model.TestIdentity(suite_id, test_id)


def decode_selection_bytes(content: bytes) -> model.FixtureSelection:
    """Decode one strict identity-only fixture selection.

    Parameters
    ----------
    content : bytes
        Exact UTF-8 YAML selection bytes.

    Returns
    -------
    model.FixtureSelection
        Validated sorted and unique fixture identities.

    Raises
    ------
    TypeError
        The document or a field has the wrong type.
    ValueError
        The document is open, malformed, empty, duplicate, or unordered.
    UnicodeDecodeError
        ``content`` is not valid UTF-8.
    yaml.YAMLError
        ``content`` is not valid strict YAML.
    """
    if type(content) is not bytes:
        raise TypeError("fixture selection content must be bytes")
    context = "semantic_fixture_selection"
    root = expect_mapping(load_yaml_text(content.decode("utf-8")), context)
    expect_exact_keys(root, {"version", "fixtures"}, context)
    identities = tuple(
        _decode_selection_identity(item, f"{context}.fixtures[{index}]")
        for index, item in enumerate(
            expect_list(root.get("fixtures"), f"{context}.fixtures")
        )
    )
    return model.FixtureSelection(
        schema_version=_required_text(root.get("version"), f"{context}.version"),
        identities=identities,
    )


def _source_payload(source: suite_model.FixtureSource) -> dict[str, JsonValue]:
    if isinstance(source, suite_model.Utf8Source):
        return {"kind": "utf8", "text": source.text}
    if isinstance(source, suite_model.RawBytesSource):
        return {"kind": "bytes", "hex": source.hex_bytes}
    return {
        "kind": "files",
        "files": {item.path: item.text for item in source.files},
        "symlinks": {item.path: item.target for item in source.symlinks},
        "entry": source.entry,
        "file_set_root": source.root,
    }


def _invocation_payload(
    invocation: suite_model.FixtureInvocation,
) -> dict[str, JsonValue]:
    return {
        "mode": invocation.mode,
        "runner": invocation.runner,
        "source": _source_payload(invocation.source),
    }


def _decode_invocation(
    value: YamlValue,
    context: str,
) -> suite_model.FixtureInvocation:
    invocation = expect_mapping(value, context)
    expect_exact_keys(invocation, {"mode", "runner", "source"}, context)
    source_context = f"{context}.source"
    source = expect_mapping(invocation.get("source"), source_context)
    kind = _required_text(source.get("kind"), f"{source_context}.kind")
    fixture: dict[str, YamlValue] = {
        "mode": _required_text(invocation.get("mode"), f"{context}.mode"),
        "runner": _required_text(invocation.get("runner"), f"{context}.runner"),
    }
    if kind == "utf8":
        expect_exact_keys(source, {"kind", "text"}, source_context)
        fixture["code"] = expect_text(
            source.get("text"),
            f"{source_context}.text",
            allow_empty=True,
        )
    elif kind == "bytes":
        expect_exact_keys(source, {"kind", "hex"}, source_context)
        fixture["code_bytes_hex"] = expect_text(
            source.get("hex"),
            f"{source_context}.hex",
            allow_empty=True,
        )
    elif kind == "files":
        expect_exact_keys(
            source,
            {"kind", "files", "symlinks", "entry", "file_set_root"},
            source_context,
        )
        fixture["files"] = expect_mapping(
            source.get("files"),
            f"{source_context}.files",
        )
        symlinks = expect_mapping(
            source.get("symlinks"),
            f"{source_context}.symlinks",
        )
        if symlinks:
            fixture["symlinks"] = symlinks
        fixture["entry"] = _required_text(
            source.get("entry"),
            f"{source_context}.entry",
        )
        root = source.get("file_set_root")
        if root is not None:
            fixture["file_set_root"] = _required_text(
                root,
                f"{source_context}.file_set_root",
            )
    else:
        raise ValueError(
            f"{source_context}.kind must be one of ['bytes', 'files', 'utf8']"
        )
    return suite_codec.decode_fixture_invocation(fixture, context)


def _primary_span_payload(
    span: suite_model.PrimarySpanAssertion | None,
) -> dict[str, JsonValue] | None:
    if span is None:
        return None
    if isinstance(span, suite_model.MarkerPrimarySpan):
        return {
            "kind": "marker",
            "marker": span.marker,
            "source": span.source,
            "line": span.line,
        }
    return {
        "kind": "bytes",
        "source": span.source,
        "bytes": {
            "start": span.byte_range.start,
            "end": span.byte_range.end,
        },
    }


def _decode_primary_span(
    value: YamlValue,
    context: str,
) -> suite_model.PrimarySpanAssertion | None:
    if value is None:
        return None
    span = expect_mapping(value, context)
    kind = _required_text(span.get("kind"), f"{context}.kind")
    if kind == "marker":
        expect_exact_keys(span, {"kind", "marker", "source", "line"}, context)
        return suite_model.MarkerPrimarySpan(
            marker=_required_text(span.get("marker"), f"{context}.marker"),
            source=_required_text(span.get("source"), f"{context}.source"),
            line=expect_positive_int(span.get("line"), f"{context}.line"),
        )
    if kind != "bytes":
        raise ValueError(f"{context}.kind must be 'bytes' or 'marker'")
    expect_exact_keys(span, {"kind", "source", "bytes"}, context)
    byte_range = expect_mapping(span.get("bytes"), f"{context}.bytes")
    expect_exact_keys(byte_range, {"start", "end"}, f"{context}.bytes")
    return suite_model.BytePrimarySpan(
        source=_required_text(span.get("source"), f"{context}.source"),
        byte_range=diagnostic_model.ByteRange(
            start=expect_non_negative_int(
                byte_range.get("start"),
                f"{context}.bytes.start",
            ),
            end=expect_non_negative_int(
                byte_range.get("end"),
                f"{context}.bytes.end",
            ),
        ),
    )


def _diagnostic_payload(
    diagnostic: suite_model.DiagnosticAssertion,
) -> dict[str, JsonValue]:
    return {
        "code": diagnostic.code,
        "message_all_of": list(diagnostic.message_all_of),
        "message_any_of": [list(group) for group in diagnostic.message_any_of],
        "data_keys": list(diagnostic.data_keys),
        "data_equals": {
            key: cast(JsonValue, value) for key, value in diagnostic.data_equals
        },
        "help_contains": (
            None if diagnostic.help_contains is None else list(diagnostic.help_contains)
        ),
        "related_contains": (
            None
            if diagnostic.related_contains is None
            else list(diagnostic.related_contains)
        ),
        "primary_span": _primary_span_payload(diagnostic.primary_span),
    }


def _decode_diagnostic(
    value: YamlValue,
    context: str,
) -> suite_model.DiagnosticAssertion:
    diagnostic = expect_mapping(value, context)
    expect_exact_keys(
        diagnostic,
        {
            "code",
            "message_all_of",
            "message_any_of",
            "data_keys",
            "data_equals",
            "help_contains",
            "related_contains",
            "primary_span",
        },
        context,
    )
    data_equals = expect_mapping(
        diagnostic.get("data_equals"),
        f"{context}.data_equals",
    )
    return suite_model.DiagnosticAssertion(
        code=_required_text(diagnostic.get("code"), f"{context}.code"),
        message_all_of=_string_tuple(
            diagnostic.get("message_all_of"),
            f"{context}.message_all_of",
        ),
        message_any_of=tuple(
            _string_tuple(group, f"{context}.message_any_of[{index}]")
            for index, group in enumerate(
                expect_list(
                    diagnostic.get("message_any_of"),
                    f"{context}.message_any_of",
                )
            )
        ),
        data_keys=_string_tuple(
            diagnostic.get("data_keys"),
            f"{context}.data_keys",
        ),
        data_equals=tuple(
            (
                _required_text(key, f"{context}.data_equals key"),
                diagnostic_contracts.expect_diagnostic_scalar(
                    raw_value,
                    f"{context}.data_equals.{key}",
                ),
            )
            for key, raw_value in sorted(data_equals.items())
        ),
        help_contains=_optional_string_tuple(
            diagnostic.get("help_contains"),
            f"{context}.help_contains",
        ),
        related_contains=_optional_string_tuple(
            diagnostic.get("related_contains"),
            f"{context}.related_contains",
        ),
        primary_span=_decode_primary_span(
            diagnostic.get("primary_span"),
            f"{context}.primary_span",
        ),
    )


def _value_payload(outcome: suite_model.ValueOutcome) -> dict[str, JsonValue]:
    return {
        "kind": outcome.kind,
        "representation": outcome.representation,
        "error_variant": outcome.error_variant,
    }


def _decode_value(value: YamlValue, context: str) -> suite_model.ValueOutcome:
    outcome = expect_mapping(value, context)
    expect_exact_keys(
        outcome,
        {"kind", "representation", "error_variant"},
        context,
    )
    representation = outcome.get("representation")
    error_variant = outcome.get("error_variant")
    return suite_model.ValueOutcome(
        kind=_required_text(outcome.get("kind"), f"{context}.kind"),
        representation=(
            None
            if representation is None
            else expect_text(
                representation,
                f"{context}.representation",
                allow_empty=True,
            )
        ),
        error_variant=(
            None
            if error_variant is None
            else _required_text(error_variant, f"{context}.error_variant")
        ),
        numeric_comparison=None,
    )


def _outcome_payload(
    outcome: suite_model.ExpectedOutcome,
) -> dict[str, JsonValue]:
    if isinstance(outcome, suite_model.ValueOutcome):
        return {"kind": "value", "value": _value_payload(outcome)}
    if isinstance(outcome, suite_model.CompileErrorOutcome):
        return {
            "kind": "compile_error",
            "diagnostic": _diagnostic_payload(outcome.diagnostic),
        }
    raise ValueError("semantic fixture outcome is outside the first-slice surface")


def _decode_outcome(
    value: YamlValue,
    context: str,
) -> suite_model.ExpectedOutcome:
    outcome = expect_mapping(value, context)
    kind = _required_text(outcome.get("kind"), f"{context}.kind")
    if kind == "value":
        expect_exact_keys(outcome, {"kind", "value"}, context)
        return _decode_value(outcome.get("value"), f"{context}.value")
    if kind == "compile_error":
        expect_exact_keys(outcome, {"kind", "diagnostic"}, context)
        return suite_model.CompileErrorOutcome(
            _decode_diagnostic(
                outcome.get("diagnostic"),
                f"{context}.diagnostic",
            )
        )
    raise ValueError(f"{context}.kind must be 'compile_error' or 'value'")


def _expectation_payload(
    expectation: suite_model.Expectation,
) -> dict[str, JsonValue]:
    return {
        "outcome": _outcome_payload(expectation.outcome),
        "value_diagnostics": [
            _diagnostic_payload(diagnostic)
            for diagnostic in expectation.value_diagnostics
        ],
        "diagnostic_contains": (
            None
            if expectation.diagnostic_contains is None
            else list(expectation.diagnostic_contains)
        ),
        "side_effects": list(expectation.side_effects),
        "process_exit": expectation.process_exit,
    }


def _decode_expectation(
    value: YamlValue,
    context: str,
) -> suite_model.Expectation:
    expectation = expect_mapping(value, context)
    expect_exact_keys(
        expectation,
        {
            "outcome",
            "value_diagnostics",
            "diagnostic_contains",
            "side_effects",
            "process_exit",
        },
        context,
    )
    process_exit = expectation.get("process_exit")
    return suite_model.Expectation(
        outcome=_decode_outcome(
            expectation.get("outcome"),
            f"{context}.outcome",
        ),
        value_diagnostics=tuple(
            _decode_diagnostic(item, f"{context}.value_diagnostics[{index}]")
            for index, item in enumerate(
                expect_list(
                    expectation.get("value_diagnostics"),
                    f"{context}.value_diagnostics",
                )
            )
        ),
        diagnostic_contains=_optional_string_tuple(
            expectation.get("diagnostic_contains"),
            f"{context}.diagnostic_contains",
        ),
        side_effects=_string_tuple(
            expectation.get("side_effects"),
            f"{context}.side_effects",
        ),
        process_exit=(
            None
            if process_exit is None
            else expect_non_negative_int(process_exit, f"{context}.process_exit")
        ),
    )


def _fixture_payload(fixture: model.SemanticFixture) -> dict[str, JsonValue]:
    return {
        "identity": {
            "suite_id": fixture.identity.suite_id,
            "test_id": fixture.identity.test_id,
        },
        "invocation": _invocation_payload(fixture.invocation),
        "expectation": _expectation_payload(fixture.expectation),
    }


def _decode_fixture(value: YamlValue, context: str) -> model.SemanticFixture:
    fixture = expect_mapping(value, context)
    expect_exact_keys(fixture, {"identity", "invocation", "expectation"}, context)
    identity = expect_mapping(fixture.get("identity"), f"{context}.identity")
    expect_exact_keys(identity, {"suite_id", "test_id"}, f"{context}.identity")
    return model.SemanticFixture(
        identity=suite_model.TestIdentity(
            _required_text(identity.get("suite_id"), f"{context}.identity.suite_id"),
            _required_text(identity.get("test_id"), f"{context}.identity.test_id"),
        ),
        invocation=_decode_invocation(
            fixture.get("invocation"),
            f"{context}.invocation",
        ),
        expectation=_decode_expectation(
            fixture.get("expectation"),
            f"{context}.expectation",
        ),
    )


def corpus_payload(corpus: model.SemanticFixtureCorpus) -> dict[str, JsonValue]:
    """Return one semantic corpus as closed canonical JSON data.

    Parameters
    ----------
    corpus : model.SemanticFixtureCorpus
        Validated generated semantic fixture corpus.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical artifact payload.
    """
    if not isinstance(corpus, model.SemanticFixtureCorpus):
        raise TypeError("semantic corpus payload requires a SemanticFixtureCorpus")
    return {
        "schema_version": corpus.schema_version,
        "input_digest": corpus.input_digest,
        "fixtures": [_fixture_payload(fixture) for fixture in corpus.fixtures],
    }


def encode_corpus(corpus: model.SemanticFixtureCorpus) -> bytes:
    """Encode one semantic corpus with deterministic reviewable JSON.

    Parameters
    ----------
    corpus : model.SemanticFixtureCorpus
        Validated generated semantic fixture corpus.

    Returns
    -------
    bytes
        UTF-8 JSON with sorted keys, two-space indentation, and final newline.
    """
    text = json.dumps(
        corpus_payload(corpus),
        ensure_ascii=False,
        indent=2,
        sort_keys=True,
    )
    return f"{text}\n".encode()


def decode_corpus_bytes(content: bytes) -> model.SemanticFixtureCorpus:
    """Decode one strict semantic fixture corpus artifact.

    Parameters
    ----------
    content : bytes
        Exact serialized corpus bytes.

    Returns
    -------
    model.SemanticFixtureCorpus
        Validated closed corpus model.

    Raises
    ------
    TypeError
        The document or a field has the wrong type.
    ValueError
        The JSON or closed corpus schema is malformed.
    UnicodeDecodeError
        ``content`` is not valid UTF-8.
    """
    context = "semantic_fixture_corpus"
    root = expect_mapping(decode_json_bytes(content, context), context)
    expect_exact_keys(root, {"schema_version", "input_digest", "fixtures"}, context)
    return model.SemanticFixtureCorpus(
        schema_version=_required_text(
            root.get("schema_version"),
            f"{context}.schema_version",
        ),
        input_digest=_required_text(
            root.get("input_digest"),
            f"{context}.input_digest",
        ),
        fixtures=tuple(
            _decode_fixture(item, f"{context}.fixtures[{index}]")
            for index, item in enumerate(
                expect_list(root.get("fixtures"), f"{context}.fixtures")
            )
        ),
    )

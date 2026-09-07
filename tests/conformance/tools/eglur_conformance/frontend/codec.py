"""Strict JSON codec for generated front-end expectation corpora."""

import json
import re

from eglur_conformance.artifact_content import (
    JsonValue,
    decode_json_bytes,
    expect_exact_keys,
    expect_list,
    expect_mapping,
    expect_non_negative_int,
    expect_positive_int,
    expect_text,
)
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.frontend import model
from eglur_conformance.yaml_document import YamlValue

_HEX_BYTES_PATTERN = re.compile(r"(?:[0-9a-f]{2})*\Z")


def _origin_payload(origin: model.FrontendOrigin) -> dict[str, JsonValue]:
    if isinstance(origin, model.InlineConformanceOrigin):
        return {
            "kind": "inline_conformance",
            "suite_id": origin.suite_id,
            "test_id": origin.test_id,
        }
    if isinstance(origin, model.FileConformanceOrigin):
        return {
            "kind": "file_conformance",
            "suite_id": origin.suite_id,
            "test_id": origin.test_id,
            "path": origin.path,
        }
    if isinstance(origin, model.RegistryConformanceOrigin):
        return {
            "kind": "registry_conformance",
            "suite_id": origin.suite_id,
            "test_id": origin.test_id,
            "package": origin.package,
            "version": origin.version,
            "path": origin.path,
        }
    return {
        "kind": "syntax_case",
        "case_id": origin.case_id,
    }


def _source_payload(source: model.FrontendSource) -> dict[str, JsonValue]:
    if isinstance(source, model.Utf8Text):
        return {"kind": "utf8", "text": source.text}
    return {"kind": "bytes", "hex": source.content.hex()}


def _expectation_payload(
    expectation: model.FrontendExpectation,
) -> dict[str, JsonValue]:
    span: dict[str, JsonValue] | None = None
    if isinstance(expectation.primary_span, model.MarkerSpanExpectation):
        span = {
            "kind": "marker",
            "marker": expectation.primary_span.marker,
            "source": expectation.primary_span.source,
            "line": expectation.primary_span.line,
        }
    elif isinstance(expectation.primary_span, model.ByteSpanExpectation):
        span = {
            "kind": "bytes",
            "source": expectation.primary_span.source,
            "bytes": {
                "start": expectation.primary_span.byte_range.start,
                "end": expectation.primary_span.byte_range.end,
            },
        }
    return {
        "kind": expectation.kind.value,
        "diagnostic_code": expectation.diagnostic_code,
        "primary_span": span,
    }


def corpus_payload(corpus: model.FrontendCorpus) -> dict[str, JsonValue]:
    """Return one corpus as closed canonical JSON data.

    Parameters
    ----------
    corpus : model.FrontendCorpus
        Validated generated corpus.

    Returns
    -------
    dict[str, JsonValue]
        Complete logical artifact payload.
    """
    return {
        "schema_version": corpus.schema_version,
        "input_digest": corpus.input_digest,
        "cases": [
            {
                "origin": _origin_payload(case.origin),
                "entrypoint": case.entrypoint.value,
                "source": _source_payload(case.source),
                "expectation": _expectation_payload(case.expectation),
                "public_examples": [
                    {
                        "document": example.document,
                        "opening_line": example.opening_line,
                    }
                    for example in case.public_examples
                ],
            }
            for case in corpus.cases
        ],
    }


def encode_corpus(corpus: model.FrontendCorpus) -> bytes:
    """Encode one corpus with deterministic reviewable JSON formatting.

    Parameters
    ----------
    corpus : model.FrontendCorpus
        Validated generated corpus.

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


def _required_text(value: YamlValue, context: str) -> str:
    return expect_text(value, context, allow_empty=False)


def _decode_origin(value: YamlValue, context: str) -> model.FrontendOrigin:
    mapping = expect_mapping(value, context)
    kind = _required_text(mapping.get("kind"), f"{context}.kind")
    if kind == "inline_conformance":
        expect_exact_keys(mapping, {"kind", "suite_id", "test_id"}, context)
        return model.InlineConformanceOrigin(
            _required_text(mapping.get("suite_id"), f"{context}.suite_id"),
            _required_text(mapping.get("test_id"), f"{context}.test_id"),
        )
    if kind == "file_conformance":
        expect_exact_keys(
            mapping,
            {"kind", "suite_id", "test_id", "path"},
            context,
        )
        return model.FileConformanceOrigin(
            _required_text(mapping.get("suite_id"), f"{context}.suite_id"),
            _required_text(mapping.get("test_id"), f"{context}.test_id"),
            _required_text(mapping.get("path"), f"{context}.path"),
        )
    if kind == "registry_conformance":
        expect_exact_keys(
            mapping,
            {
                "kind",
                "suite_id",
                "test_id",
                "package",
                "version",
                "path",
            },
            context,
        )
        return model.RegistryConformanceOrigin(
            _required_text(mapping.get("suite_id"), f"{context}.suite_id"),
            _required_text(mapping.get("test_id"), f"{context}.test_id"),
            _required_text(mapping.get("package"), f"{context}.package"),
            _required_text(mapping.get("version"), f"{context}.version"),
            _required_text(mapping.get("path"), f"{context}.path"),
        )
    if kind == "syntax_case":
        expect_exact_keys(mapping, {"kind", "case_id"}, context)
        return model.SyntaxCaseOrigin(
            _required_text(mapping.get("case_id"), f"{context}.case_id"),
        )
    raise ValueError(f"{context}.kind has unsupported value {kind!r}")


def _decode_source(value: YamlValue, context: str) -> model.FrontendSource:
    mapping = expect_mapping(value, context)
    kind = _required_text(mapping.get("kind"), f"{context}.kind")
    if kind == "utf8":
        expect_exact_keys(mapping, {"kind", "text"}, context)
        return model.Utf8Text(
            expect_text(
                mapping.get("text"),
                f"{context}.text",
                allow_empty=True,
            )
        )
    if kind == "bytes":
        expect_exact_keys(mapping, {"kind", "hex"}, context)
        encoded = expect_text(
            mapping.get("hex"),
            f"{context}.hex",
            allow_empty=True,
        )
        if _HEX_BYTES_PATTERN.fullmatch(encoded) is None:
            raise ValueError(f"{context}.hex must be canonical lowercase byte hex")
        return model.RawBytes(bytes.fromhex(encoded))
    raise ValueError(f"{context}.kind has unsupported value {kind!r}")


def _decode_span(value: YamlValue, context: str) -> model.SpanExpectation | None:
    if value is None:
        return None
    mapping = expect_mapping(value, context)
    kind = _required_text(mapping.get("kind"), f"{context}.kind")
    if kind == "marker":
        expect_exact_keys(mapping, {"kind", "marker", "source", "line"}, context)
        return model.MarkerSpanExpectation(
            _required_text(mapping.get("marker"), f"{context}.marker"),
            _required_text(mapping.get("source"), f"{context}.source"),
            expect_positive_int(mapping.get("line"), f"{context}.line"),
        )
    if kind != "bytes":
        raise ValueError(f"{context}.kind must be 'marker' or 'bytes'")
    expect_exact_keys(mapping, {"kind", "source", "bytes"}, context)
    byte_range = expect_mapping(mapping.get("bytes"), f"{context}.bytes")
    expect_exact_keys(byte_range, {"start", "end"}, f"{context}.bytes")
    return model.ByteSpanExpectation(
        _required_text(mapping.get("source"), f"{context}.source"),
        diagnostic_model.ByteRange(
            expect_non_negative_int(
                byte_range.get("start"),
                f"{context}.bytes.start",
            ),
            expect_non_negative_int(
                byte_range.get("end"),
                f"{context}.bytes.end",
            ),
        ),
    )


def _decode_expectation(
    value: YamlValue,
    context: str,
) -> model.FrontendExpectation:
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"kind", "diagnostic_code", "primary_span"},
        context,
    )
    code_value = mapping.get("diagnostic_code")
    return model.FrontendExpectation(
        model.OutcomeKind(_required_text(mapping.get("kind"), f"{context}.kind")),
        (
            None
            if code_value is None
            else _required_text(code_value, f"{context}.diagnostic_code")
        ),
        _decode_span(mapping.get("primary_span"), f"{context}.primary_span"),
    )


def _decode_public_examples(
    value: YamlValue,
    context: str,
) -> tuple[model.PublicExample, ...]:
    examples: list[model.PublicExample] = []
    for index, item in enumerate(expect_list(value, context)):
        item_context = f"{context}[{index}]"
        mapping = expect_mapping(item, item_context)
        expect_exact_keys(mapping, {"document", "opening_line"}, item_context)
        examples.append(
            model.PublicExample(
                _required_text(
                    mapping.get("document"),
                    f"{item_context}.document",
                ),
                expect_positive_int(
                    mapping.get("opening_line"),
                    f"{item_context}.opening_line",
                ),
            )
        )
    return tuple(examples)


def _decode_case(value: YamlValue, context: str) -> model.FrontendCase:
    mapping = expect_mapping(value, context)
    expect_exact_keys(
        mapping,
        {"origin", "entrypoint", "source", "expectation", "public_examples"},
        context,
    )
    return model.FrontendCase(
        origin=_decode_origin(mapping.get("origin"), f"{context}.origin"),
        entrypoint=model.EntryPoint(
            _required_text(mapping.get("entrypoint"), f"{context}.entrypoint")
        ),
        source=_decode_source(mapping.get("source"), f"{context}.source"),
        expectation=_decode_expectation(
            mapping.get("expectation"),
            f"{context}.expectation",
        ),
        public_examples=_decode_public_examples(
            mapping.get("public_examples"),
            f"{context}.public_examples",
        ),
    )


def decode_corpus_bytes(content: bytes) -> model.FrontendCorpus:
    """Decode one strict front-end corpus artifact.

    Parameters
    ----------
    content : bytes
        Exact serialized corpus bytes.

    Returns
    -------
    model.FrontendCorpus
        Validated closed corpus model.
    """
    context = "front_end_corpus"
    root = expect_mapping(decode_json_bytes(content, context), context)
    expect_exact_keys(root, {"schema_version", "input_digest", "cases"}, context)
    return model.FrontendCorpus(
        schema_version=_required_text(
            root.get("schema_version"),
            f"{context}.schema_version",
        ),
        input_digest=_required_text(
            root.get("input_digest"),
            f"{context}.input_digest",
        ),
        cases=tuple(
            _decode_case(item, f"{context}.cases[{index}]")
            for index, item in enumerate(
                expect_list(root.get("cases"), f"{context}.cases")
            )
        ),
    )

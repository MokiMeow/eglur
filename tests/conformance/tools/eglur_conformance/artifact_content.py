"""Canonical encoding, addressing, and primitive artifact-field narrowing."""

import hashlib
import json
import re
from typing import TypeAlias, cast

from eglur_conformance.yaml_document import YamlValue

JsonValue: TypeAlias = (
    str | int | bool | None | list["JsonValue"] | dict[str, "JsonValue"]
)

SCHEMA_VERSION = "0.1"
DIGEST_PATTERN = re.compile(r"sha256:[0-9a-f]{64}\Z")
PLACEHOLDER_VALUES = frozenset({"n/a", "none", "placeholder", "tbd", "todo", "unknown"})


def expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    """Return a YAML mapping with string keys."""
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise TypeError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def expect_list(value: YamlValue, context: str) -> list[YamlValue]:
    """Return a YAML list."""
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def expect_exact_keys(
    mapping: dict[str, YamlValue],
    required: set[str],
    context: str,
) -> None:
    """Require one closed mapping shape."""
    missing = sorted(required - set(mapping))
    unsupported = sorted(set(mapping) - required)
    if missing or unsupported:
        details: list[str] = []
        if missing:
            details.append(f"missing {missing}")
        if unsupported:
            details.append(f"unsupported {unsupported}")
        raise ValueError(f"{context} has invalid keys: {', '.join(details)}")


def expect_concrete_string(value: YamlValue, context: str) -> str:
    """Return a trimmed, non-placeholder YAML string."""
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if value == "" or value != value.strip():
        raise ValueError(f"{context} must be non-empty and have no outer whitespace")
    if any(0xD800 <= ord(character) <= 0xDFFF for character in value):
        raise ValueError(f"{context} must not contain lone surrogate code points")
    if value.casefold() in PLACEHOLDER_VALUES or (
        value.startswith("<") and value.endswith(">")
    ):
        raise ValueError(f"{context} must be concrete")
    return value


def expect_text(
    value: YamlValue,
    context: str,
    *,
    allow_empty: bool,
) -> str:
    """Validate Unicode scalar text without applying placeholder rules.

    Parameters
    ----------
    value : YamlValue
        Candidate YAML scalar.
    context : str
        Field name used in validation errors.
    allow_empty : bool
        Whether the empty string is valid.

    Returns
    -------
    str
        The validated text without normalization.

    Raises
    ------
    TypeError
        If ``value`` is not a string.
    ValueError
        If empty text is forbidden or the text contains a lone surrogate.
    """
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if not allow_empty and value == "":
        raise ValueError(f"{context} must not be empty")
    if any(0xD800 <= ord(character) <= 0xDFFF for character in value):
        raise ValueError(f"{context} must not contain lone surrogate code points")
    return value


def expect_positive_int(value: YamlValue, context: str) -> int:
    """Return a strictly positive YAML integer."""
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{context} must be an integer")
    if value <= 0:
        raise ValueError(f"{context} must be positive")
    return value


def expect_non_negative_int(value: YamlValue, context: str) -> int:
    """Return a non-negative YAML integer."""
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{context} must be an integer")
    if value < 0:
        raise ValueError(f"{context} must be non-negative")
    return value


def expect_digest(value: YamlValue, context: str) -> str:
    """Return one canonical SHA-256 content address."""
    digest = expect_concrete_string(value, context)
    if DIGEST_PATTERN.fullmatch(digest) is None:
        raise ValueError(f"{context} must be sha256:<64 lowercase hex digits>")
    return digest


def canonical_json_bytes(value: JsonValue) -> bytes:
    """Encode the closed evidence domain with its exact canonical JSON rules."""
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def canonical_digest(value: JsonValue) -> str:
    """Address logical evidence with canonical JSON bytes."""
    return f"sha256:{hashlib.sha256(canonical_json_bytes(value)).hexdigest()}"


def raw_artifact_digest(content: bytes) -> str:
    """Return the SHA-256 address of immutable raw result, log, or decision bytes."""
    if not isinstance(content, bytes):
        raise TypeError("raw artifact content must be bytes")
    return f"sha256:{hashlib.sha256(content).hexdigest()}"


def snapshot_bytes(content: bytes, context: str) -> bytes:
    """Copy exact bytes into an immutable snapshot.

    Parameters
    ----------
    content : bytes
        Candidate serialized artifact content.
    context : str
        Field name used in validation errors.

    Returns
    -------
    bytes
        A stable copy preserving the exact byte sequence.

    Raises
    ------
    TypeError
        If ``content`` is not bytes.
    """
    if not isinstance(content, bytes):
        raise TypeError(f"{context} must be bytes")
    return memoryview(content).tobytes()


def _reject_duplicate_json_pairs(
    pairs: list[tuple[str, YamlValue]],
) -> dict[str, YamlValue]:
    mapping: dict[str, YamlValue] = {}
    for key, value in pairs:
        if key in mapping:
            raise ValueError(f"JSON object repeats key {key!r}")
        mapping[key] = value
    return mapping


def _reject_non_json_constant(value: str) -> None:
    raise ValueError(f"JSON contains unsupported number {value}")


def decode_json_bytes(content: bytes, context: str) -> YamlValue:
    """Decode strict UTF-8 JSON without accepting duplicate keys.

    Parameters
    ----------
    content : bytes
        Exact serialized JSON bytes.
    context : str
        Artifact identity used in decoding errors.

    Returns
    -------
    YamlValue
        Parsed JSON data for subsequent closed-schema decoding.

    Raises
    ------
    TypeError
        ``content`` is not bytes.
    ValueError
        The JSON is malformed, repeats a key, or uses a non-JSON number.
    UnicodeDecodeError
        ``content`` is not valid UTF-8.
    """
    if type(content) is not bytes:
        raise TypeError(f"{context} must be bytes")
    return cast(
        YamlValue,
        json.loads(
            content.decode("utf-8"),
            object_pairs_hook=_reject_duplicate_json_pairs,
            parse_constant=_reject_non_json_constant,
        ),
    )

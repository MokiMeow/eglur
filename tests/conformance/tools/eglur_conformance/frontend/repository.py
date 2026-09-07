"""Repository adapters for canonical front-end inputs and generated output."""

import os
import re
import tempfile
from dataclasses import dataclass
from pathlib import Path

from eglur_conformance import markdown_document
from eglur_conformance.artifact_content import (
    JsonValue,
    canonical_digest,
    decode_json_bytes,
    expect_exact_keys,
    expect_list,
    expect_mapping,
    expect_text,
    raw_artifact_digest,
)
from eglur_conformance.documentation import repository as documentation_repository
from eglur_conformance.frontend import codec, model, projection
from eglur_conformance.suite import repository as suite_repository
from eglur_conformance.yaml_document import YamlValue

CORPUS_PATH = Path("tests/frontend/expectations.json")
_SYNTAX_CASES_PATH = Path("tests/frontend/syntax-cases.json")
_FIXTURE_INFO = re.compile(r"eglur fixture=(?P<fixture_id>[A-Za-z0-9-]+)\Z")


@dataclass(frozen=True)
class _PublicInputs:
    bindings: tuple[model.PublicExampleBinding, ...]
    document_bytes: dict[str, bytes]


def _resolved_repository_root(repo_root: Path) -> Path:
    if not isinstance(repo_root, Path):
        raise TypeError("repository root must be a Path")
    resolved = repo_root.resolve(strict=True)
    if not resolved.is_dir():
        raise ValueError(f"repository root is not a directory: {repo_root}")
    return resolved


def _decode_syntax_expectation(
    value: YamlValue,
    context: str,
) -> model.FrontendExpectation:
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"kind", "diagnostic_code"}, context)
    kind = model.OutcomeKind(
        expect_text(mapping.get("kind"), f"{context}.kind", allow_empty=False)
    )
    raw_diagnostic = mapping.get("diagnostic_code")
    diagnostic = (
        None
        if raw_diagnostic is None
        else expect_text(
            raw_diagnostic,
            f"{context}.diagnostic_code",
            allow_empty=False,
        )
    )
    return model.FrontendExpectation(kind, diagnostic, None)


def _decode_syntax_case(value: YamlValue, index: int) -> model.SyntaxCase:
    context = f"syntax_cases.cases[{index}]"
    mapping = expect_mapping(value, context)
    expect_exact_keys(mapping, {"id", "entrypoint", "source", "expectation"}, context)
    return model.SyntaxCase(
        case_id=expect_text(
            mapping.get("id"),
            f"{context}.id",
            allow_empty=False,
        ),
        entrypoint=model.EntryPoint(
            expect_text(
                mapping.get("entrypoint"),
                f"{context}.entrypoint",
                allow_empty=False,
            )
        ),
        source=expect_text(
            mapping.get("source"),
            f"{context}.source",
            allow_empty=False,
        ),
        expectation=_decode_syntax_expectation(
            mapping.get("expectation"),
            f"{context}.expectation",
        ),
    )


def decode_syntax_cases(content: bytes) -> tuple[model.SyntaxCase, ...]:
    """Decode authored focused syntax cases through their strict JSON contract.

    Parameters
    ----------
    content : bytes
        Exact syntax-case bytes.

    Returns
    -------
    tuple[model.SyntaxCase, ...]
        Validated syntax cases in source order.
    """
    context = "syntax_cases"
    root = expect_mapping(decode_json_bytes(content, context), context)
    expect_exact_keys(root, {"version", "cases"}, context)
    version = expect_text(
        root.get("version"),
        f"{context}.version",
        allow_empty=False,
    )
    if version != model.SCHEMA_VERSION:
        raise ValueError(
            f"{context}.version must be {model.SCHEMA_VERSION!r}, got {version!r}"
        )
    raw_cases = expect_list(root.get("cases"), f"{context}.cases")
    if not raw_cases:
        raise ValueError(f"{context}.cases must not be empty")
    cases = tuple(
        _decode_syntax_case(item, index) for index, item in enumerate(raw_cases)
    )
    identities = tuple(case.case_id for case in cases)
    if len(set(identities)) != len(identities):
        raise ValueError(f"{context} case ids must be unique")
    return cases


def _load_public_inputs(repo_root: Path) -> _PublicInputs:
    documentation = documentation_repository.load_tracked_documentation(repo_root)
    bindings: list[model.PublicExampleBinding] = []
    document_bytes: dict[str, bytes] = {}
    for source in documentation.sources:
        path = repo_root.joinpath(*source.path.parts)
        linked = False
        for block in markdown_document.fenced_code_blocks(path):
            if not block.info.startswith("eglur fixture"):
                continue
            matched = _FIXTURE_INFO.fullmatch(block.info)
            if matched is None:
                raise ValueError(
                    f"{source.path.as_posix()}:{block.opening_line}: "
                    "invalid Eglur fixture info"
                )
            bindings.append(
                model.PublicExampleBinding(
                    fixture_id=matched.group("fixture_id"),
                    document=source.path.as_posix(),
                    opening_line=block.opening_line,
                    source=block.code,
                )
            )
            linked = True
        if linked:
            document_bytes[source.path.as_posix()] = path.read_bytes()
    return _PublicInputs(
        bindings=tuple(
            sorted(
                bindings,
                key=lambda binding: (binding.document, binding.opening_line),
            )
        ),
        document_bytes=document_bytes,
    )


def _input_digest(
    conformance: suite_repository.ConformanceInputs,
    syntax_case_bytes: bytes,
    public: _PublicInputs,
) -> str:
    public_digests: dict[str, JsonValue] = {
        path: raw_artifact_digest(content)
        for path, content in public.document_bytes.items()
    }
    return canonical_digest(
        {
            "conformance_bundle": conformance.content_digest,
            "syntax_cases": raw_artifact_digest(syntax_case_bytes),
            "public_documents": public_digests,
        }
    )


def build_corpus(repo_root: Path) -> model.FrontendCorpus:
    """Build the front-end corpus from exact authoritative repository inputs.

    Parameters
    ----------
    repo_root : pathlib.Path
        Git worktree root.

    Returns
    -------
    model.FrontendCorpus
        Deterministic generated corpus.
    """
    root = _resolved_repository_root(repo_root)
    conformance = suite_repository.load_conformance_inputs(root)
    syntax_case_bytes = (root / _SYNTAX_CASES_PATH).read_bytes()
    syntax_cases = decode_syntax_cases(syntax_case_bytes)
    public = _load_public_inputs(root)
    return projection.project_corpus(
        conformance.bundle,
        syntax_cases,
        public.bindings,
        _input_digest(conformance, syntax_case_bytes, public),
    )


def generated_corpus_bytes(repo_root: Path) -> bytes:
    """Return exact generated artifact bytes for one repository.

    Parameters
    ----------
    repo_root : pathlib.Path
        Git worktree root.

    Returns
    -------
    bytes
        Deterministically encoded corpus bytes.
    """
    return codec.encode_corpus(build_corpus(repo_root))


def corpus_is_current(repo_root: Path) -> bool:
    """Return whether the checked-in corpus equals exact regeneration.

    Parameters
    ----------
    repo_root : pathlib.Path
        Git worktree root.

    Returns
    -------
    bool
        True only when the generated artifact exists and is byte-exact.
    """
    root = _resolved_repository_root(repo_root)
    try:
        current = (root / CORPUS_PATH).read_bytes()
    except FileNotFoundError:
        return False
    return current == generated_corpus_bytes(root)


def write_corpus(repo_root: Path) -> None:
    """Atomically replace the checked-in corpus with exact regeneration.

    Parameters
    ----------
    repo_root : pathlib.Path
        Git worktree root.
    """
    root = _resolved_repository_root(repo_root)
    output_path = root / CORPUS_PATH
    output_path.parent.mkdir(parents=True, exist_ok=True)
    content = generated_corpus_bytes(root)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=output_path.parent,
        prefix=f".{output_path.name}.",
        suffix=".tmp",
    )
    temporary_path = Path(temporary_name)
    try:
        os.fchmod(descriptor, 0o644)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary_path, output_path)
    finally:
        try:
            os.close(descriptor)
        except OSError:
            pass
        temporary_path.unlink(missing_ok=True)

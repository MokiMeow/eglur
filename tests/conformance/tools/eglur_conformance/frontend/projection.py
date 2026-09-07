"""Pure projection from canonical fixtures to front-end expectations."""

from dataclasses import replace

from eglur_conformance.frontend import model
from eglur_conformance.suite import model as suite_model


def _span_expectation(
    diagnostic: suite_model.DiagnosticAssertion,
) -> model.SpanExpectation | None:
    span = diagnostic.primary_span
    if span is None:
        return None
    if isinstance(span, suite_model.MarkerPrimarySpan):
        return model.MarkerSpanExpectation(span.marker, span.source, span.line)
    return model.ByteSpanExpectation(span.source, span.byte_range)


def _front_end_expectation(
    outcome: suite_model.ExpectedOutcome,
) -> model.FrontendExpectation:
    if not isinstance(outcome, suite_model.CompileErrorOutcome):
        return model.FrontendExpectation(model.OutcomeKind.SUCCESS, None, None)

    diagnostic = outcome.diagnostic
    if diagnostic.code.startswith("LEX_"):
        return model.FrontendExpectation(
            model.OutcomeKind.LEXICAL_FAILURE,
            diagnostic.code,
            _span_expectation(diagnostic),
        )
    if diagnostic.code.startswith("PARSE_"):
        return model.FrontendExpectation(
            model.OutcomeKind.PARSE_FAILURE,
            diagnostic.code,
            _span_expectation(diagnostic),
        )
    return model.FrontendExpectation(model.OutcomeKind.SUCCESS, None, None)


def _conformance_identity(
    test: suite_model.ConformanceTest,
) -> tuple[str, str]:
    return test.suite.suite_id, test.case.test_id


def _inline_case(
    test: suite_model.ConformanceTest,
    source: suite_model.Utf8Source,
) -> model.FrontendCase:
    suite_id, test_id = _conformance_identity(test)
    return model.FrontendCase(
        origin=model.InlineConformanceOrigin(suite_id, test_id),
        entrypoint=model.EntryPoint.COMPILATION_UNIT,
        source=model.Utf8Text(source.text),
        expectation=_front_end_expectation(test.case.expectation.outcome),
    )


def _raw_bytes_case(
    test: suite_model.ConformanceTest,
    source: suite_model.RawBytesSource,
) -> model.FrontendCase:
    suite_id, test_id = _conformance_identity(test)
    return model.FrontendCase(
        origin=model.InlineConformanceOrigin(suite_id, test_id),
        entrypoint=model.EntryPoint.COMPILATION_UNIT,
        source=model.RawBytes(bytes.fromhex(source.hex_bytes)),
        expectation=_front_end_expectation(test.case.expectation.outcome),
    )


def _file_expectation(
    expectation: model.FrontendExpectation,
    path: str,
    source_paths: frozenset[str],
) -> model.FrontendExpectation:
    if expectation.kind is model.OutcomeKind.SUCCESS:
        return expectation
    span = expectation.primary_span
    if span is None or span.source not in source_paths:
        raise ValueError(
            "file-set front-end failure must identify one canonical Eglur source"
        )
    if span.source == path:
        return expectation
    return model.FrontendExpectation(model.OutcomeKind.SUCCESS, None, None)


def _file_set_cases(
    test: suite_model.ConformanceTest,
    source: suite_model.FileSetSource,
) -> tuple[model.FrontendCase, ...]:
    source_files = tuple(item for item in source.files if item.path.endswith(".eglur"))
    source_paths = frozenset(item.path for item in source_files)
    expectation = _front_end_expectation(test.case.expectation.outcome)
    suite_id, test_id = _conformance_identity(test)
    return tuple(
        model.FrontendCase(
            origin=model.FileConformanceOrigin(suite_id, test_id, item.path),
            entrypoint=model.EntryPoint.COMPILATION_UNIT,
            source=model.Utf8Text(item.text),
            expectation=_file_expectation(expectation, item.path, source_paths),
        )
        for item in source_files
    )


def _registry_path(path: suite_model.RegistryBytes, context: str) -> str:
    if isinstance(path, suite_model.RegistryRepeatedByte):
        raise TypeError(f"{context} uses an unexpanded repeated-byte path")
    try:
        return path.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{context} path is not valid UTF-8") from error


def _registry_source(
    entry: suite_model.RegistryFileEntry,
    context: str,
) -> tuple[str, str] | None:
    path = _registry_path(entry.path, context)
    if not path.endswith(".eglur"):
        return None
    if isinstance(entry.content, suite_model.RegistryRepeatedByte):
        raise TypeError(f"{context} uses unexpanded repeated source bytes")
    try:
        return path, entry.content.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{context} source is not valid UTF-8") from error


def _registry_step_source(
    step: suite_model.RegistryProviderStep,
    context: str,
) -> tuple[str, str] | None:
    if isinstance(step, suite_model.RegistryFinishStep):
        return None
    entry = step.entry
    if not isinstance(entry, suite_model.RegistryFileEntry):
        return None
    source = _registry_source(entry, context)
    if source is not None and isinstance(step, suite_model.RegistryRepeatStep):
        raise ValueError(f"{context} repeats one registry source identity")
    return source


def _registry_cases(
    test: suite_model.ConformanceTest,
) -> tuple[model.FrontendCase, ...]:
    if isinstance(test.case.expectation.outcome, suite_model.CompileErrorOutcome):
        return ()

    suite_id, test_id = _conformance_identity(test)
    cases: list[model.FrontendCase] = []
    for acquisition_index, acquisition in enumerate(
        test.case.invocation.registry_acquisitions
    ):
        finish = acquisition.provider_steps[-1]
        if not isinstance(finish, suite_model.RegistryFinishStep):
            raise TypeError("registry acquisition has no terminal finish step")
        if finish.outcome != "complete":
            continue
        for step_index, step in enumerate(acquisition.provider_steps[:-1]):
            context = (
                f"{suite_id}/{test_id} registry acquisition {acquisition_index} "
                f"step {step_index}"
            )
            source = _registry_step_source(step, context)
            if source is None:
                continue
            path, text = source
            cases.append(
                model.FrontendCase(
                    origin=model.RegistryConformanceOrigin(
                        suite_id,
                        test_id,
                        acquisition.package,
                        acquisition.version,
                        path,
                    ),
                    entrypoint=model.EntryPoint.COMPILATION_UNIT,
                    source=model.Utf8Text(text),
                    expectation=model.FrontendExpectation(
                        model.OutcomeKind.SUCCESS,
                        None,
                        None,
                    ),
                )
            )
    return tuple(cases)


def _conformance_cases(
    test: suite_model.ConformanceTest,
) -> tuple[model.FrontendCase, ...]:
    source = test.case.invocation.source
    if isinstance(source, suite_model.Utf8Source):
        root_cases = (_inline_case(test, source),)
    elif isinstance(source, suite_model.RawBytesSource):
        root_cases = (_raw_bytes_case(test, source),)
    else:
        root_cases = _file_set_cases(test, source)
    return (*root_cases, *_registry_cases(test))


def _syntax_case(case: model.SyntaxCase) -> model.FrontendCase:
    return model.FrontendCase(
        origin=model.SyntaxCaseOrigin(case.case_id),
        entrypoint=case.entrypoint,
        source=model.Utf8Text(case.source),
        expectation=case.expectation,
    )


def _attach_public_examples(
    cases: tuple[model.FrontendCase, ...],
    bindings: tuple[model.PublicExampleBinding, ...],
    required_inline_identities: frozenset[tuple[str, str]],
) -> tuple[model.FrontendCase, ...]:
    inline_by_test_id: dict[str, int] = {}
    for index, case in enumerate(cases):
        origin = case.origin
        if not isinstance(origin, model.InlineConformanceOrigin):
            continue
        if (origin.suite_id, origin.test_id) not in required_inline_identities:
            continue
        if origin.test_id in inline_by_test_id:
            raise ValueError(
                f"public fixture id is not globally unique: {origin.test_id}"
            )
        inline_by_test_id[origin.test_id] = index

    locations: dict[int, list[model.PublicExample]] = {}
    for binding in bindings:
        index = inline_by_test_id.get(binding.fixture_id)
        if index is None:
            raise ValueError(
                f"public fixture does not name an inline conformance source: "
                f"{binding.fixture_id}"
            )
        case = cases[index]
        if not isinstance(case.source, model.Utf8Text):
            raise TypeError("public fixture source is not UTF-8 text")
        if binding.source != case.source.text:
            raise ValueError(
                f"public fixture source drift: {binding.document}:"
                f"{binding.opening_line} ({binding.fixture_id})"
            )
        locations.setdefault(index, []).append(
            model.PublicExample(binding.document, binding.opening_line)
        )

    return tuple(
        replace(case, public_examples=tuple(sorted(locations.get(index, ()))))
        for index, case in enumerate(cases)
    )


def project_corpus(
    bundle: suite_model.ConformanceBundle,
    syntax_cases: tuple[model.SyntaxCase, ...],
    public_bindings: tuple[model.PublicExampleBinding, ...],
    input_digest: str,
) -> model.FrontendCorpus:
    """Project canonical inputs into one deterministic front-end corpus.

    Parameters
    ----------
    bundle : suite_model.ConformanceBundle
        Exact manifest-owned conformance suite model.
    syntax_cases : tuple[model.SyntaxCase, ...]
        Strictly decoded focused syntax cases.
    public_bindings : tuple[model.PublicExampleBinding, ...]
        Tracked public examples linked to conformance fixture IDs.
    input_digest : str
        Content address of all authoritative input bytes.

    Returns
    -------
    model.FrontendCorpus
        Sorted source units with phase-correct front-end expectations.
    """
    if not isinstance(bundle, suite_model.ConformanceBundle):
        raise TypeError("front-end bundle must be a ConformanceBundle")
    if type(syntax_cases) is not tuple or not all(
        isinstance(case, model.SyntaxCase) for case in syntax_cases
    ):
        raise TypeError("front-end syntax_cases must be a tuple of SyntaxCase values")
    if type(public_bindings) is not tuple or not all(
        isinstance(binding, model.PublicExampleBinding) for binding in public_bindings
    ):
        raise TypeError(
            "front-end public_bindings must be a tuple of PublicExampleBinding values"
        )

    conformance_cases = tuple(
        case for test in bundle.tests for case in _conformance_cases(test)
    )
    cases = (*conformance_cases, *(_syntax_case(case) for case in syntax_cases))
    required_inline_identities = frozenset(
        (test.suite.suite_id, test.case.test_id)
        for test in bundle.tests
        if test.suite.required
        and isinstance(test.case.invocation.source, suite_model.Utf8Source)
    )
    with_examples = _attach_public_examples(
        cases,
        public_bindings,
        required_inline_identities,
    )
    return model.FrontendCorpus(
        schema_version=model.SCHEMA_VERSION,
        input_digest=input_digest,
        cases=tuple(sorted(with_examples, key=lambda case: case.identity)),
    )

"""Decode strict YAML values into canonical conformance suite models."""

from collections import Counter
from collections.abc import Mapping
from typing import cast

from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.diagnostics import model as diagnostic_model
from eglur_conformance.suite import model, schema
from eglur_conformance.validation import ValidationProblem
from eglur_conformance.yaml_document import YamlValue


def _mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def _list(value: YamlValue, context: str) -> list[YamlValue]:
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def _text(value: YamlValue, context: str, *, allow_empty: bool = False) -> str:
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if not allow_empty and value == "":
        raise ValueError(f"{context} must be non-empty")
    return value


def _integer(value: YamlValue, context: str) -> int:
    if type(value) is not int:
        raise TypeError(f"{context} must be an integer")
    return value


def _string_tuple(value: YamlValue, context: str) -> tuple[str, ...]:
    return tuple(
        _text(item, f"{context}[{index}]", allow_empty=True)
        for index, item in enumerate(_list(value, context))
    )


def decode_fixture_invocation(
    value: Mapping[str, YamlValue],
    context: str = "fixture",
) -> model.FixtureInvocation:
    """Decode source and runner fields after applying source-form invariants."""
    test = dict(value)
    source_problems = schema.validate_source_form(test, context)
    if source_problems:
        raise ValueError("; ".join(problem.message for problem in source_problems))

    raw_mode = test.get("mode", "auto")
    mode = _text(raw_mode, f"{context}.mode")
    if mode not in schema.ALLOWED_MODES:
        raise ValueError(f"{context}.mode is unsupported: {mode}")
    raw_runner = test.get("runner", "main")
    runner = _text(raw_runner, f"{context}.runner")
    if runner not in schema.ALLOWED_RUNNERS:
        raise ValueError(f"{context}.runner is unsupported: {runner}")

    if "code" in test:
        source: model.FixtureSource = model.Utf8Source(
            _text(test["code"], f"{context}.code", allow_empty=True)
        )
    elif "code_bytes_hex" in test:
        source = model.RawBytesSource(
            _text(test["code_bytes_hex"], f"{context}.code_bytes_hex").lower()
        )
    else:
        files = _mapping(test["files"], f"{context}.files")
        symlinks = _mapping(test.get("symlinks", {}), f"{context}.symlinks")
        source = model.FileSetSource(
            files=tuple(
                model.SourceFile(
                    path=path,
                    text=_text(
                        text,
                        f"{context}.files.{path}",
                        allow_empty=True,
                    ),
                )
                for path, text in sorted(files.items())
            ),
            symlinks=tuple(
                model.FixtureSymlink(
                    path=path,
                    target=_text(target, f"{context}.symlinks.{path}"),
                )
                for path, target in sorted(symlinks.items())
            ),
            entry=_text(test["entry"], f"{context}.entry"),
            root=(
                None
                if "file_set_root" not in test
                else _text(test["file_set_root"], f"{context}.file_set_root")
            ),
        )

    registry_acquisitions = tuple(
        sorted(
            (
                _decode_registry_acquisition(
                    item, f"{context}.registry_acquisitions[{index}]"
                )
                for index, item in enumerate(
                    _list(
                        test.get("registry_acquisitions", []),
                        f"{context}.registry_acquisitions",
                    )
                )
            ),
            key=lambda item: (item.package, item.version),
        )
    )
    return model.FixtureInvocation(
        source=source,
        registry_acquisitions=registry_acquisitions,
        mode=cast(model.FixtureMode, mode),
        runner=cast(model.FixtureRunner, runner),
    )


def _decode_registry_bytes(
    value: dict[str, YamlValue],
    text_key: str,
    hex_key: str,
    repeat_key: str,
    context: str,
) -> model.RegistryBytes:
    if text_key in value:
        return _text(
            value[text_key],
            f"{context}.{text_key}",
            allow_empty=True,
        ).encode("utf-8")
    if hex_key in value:
        return bytes.fromhex(
            _text(
                value[hex_key],
                f"{context}.{hex_key}",
                allow_empty=True,
            )
        )
    repeated = _mapping(value[repeat_key], f"{context}.{repeat_key}")
    return model.RegistryRepeatedByte(
        byte=int(
            _text(repeated["byte_hex"], f"{context}.{repeat_key}.byte_hex"),
            16,
        ),
        count=_integer(repeated["count"], f"{context}.{repeat_key}.count"),
    )


def _decode_registry_entry(
    value: YamlValue,
    context: str,
) -> model.RegistryEntry:
    entry = _mapping(value, context)
    kind = _text(entry["kind"], f"{context}.kind")
    path = _decode_registry_bytes(entry, "path", "path_hex", "path_repeat", context)
    if kind == "file":
        return model.RegistryFileEntry(
            path=path,
            content=_decode_registry_bytes(
                entry,
                "text",
                "content_hex",
                "content_repeat",
                context,
            ),
        )
    if kind == "directory":
        return model.RegistryDirectoryEntry(path=path)
    if kind in ("symlink", "hardlink"):
        return model.RegistryLinkEntry(
            kind=kind,
            path=path,
            target=_decode_registry_bytes(
                entry,
                "target",
                "target_hex",
                "target_repeat",
                context,
            ),
        )
    return model.RegistrySpecialEntry(
        kind=cast(model.RegistrySpecialKind, kind),
        path=path,
    )


def _decode_registry_provider_step(
    value: YamlValue,
    context: str,
) -> model.RegistryProviderStep:
    step = _mapping(value, context)
    if "emit" in step:
        return model.RegistryEmitStep(
            _decode_registry_entry(step["emit"], f"{context}.emit")
        )
    if "repeat" in step:
        repeated = _mapping(step["repeat"], f"{context}.repeat")
        return model.RegistryRepeatStep(
            count=_integer(repeated["count"], f"{context}.repeat.count"),
            entry=_decode_registry_entry(
                repeated["emit"],
                f"{context}.repeat.emit",
            ),
        )
    return model.RegistryFinishStep(
        cast(
            model.RegistryProviderFinish,
            _text(step["finish"], f"{context}.finish"),
        )
    )


def _decode_registry_acquisition(
    value: YamlValue,
    context: str,
) -> model.RegistryAcquisition:
    """Decode one schema-validated transactional provider attempt."""
    acquisition = _mapping(value, context)
    return model.RegistryAcquisition(
        package=_text(acquisition["package"], f"{context}.package"),
        version=_text(acquisition["version"], f"{context}.version"),
        provider_steps=tuple(
            _decode_registry_provider_step(step, f"{context}.provider_steps[{index}]")
            for index, step in enumerate(
                _list(acquisition["provider_steps"], f"{context}.provider_steps")
            )
        ),
    )


def _decode_diagnostic(
    value: YamlValue,
    test: dict[str, schema.YamlValue],
    context: str,
) -> model.DiagnosticAssertion:
    diagnostic = _mapping(value, context)
    primary_span: model.PrimarySpanAssertion | None = None
    if "primary_span" in diagnostic:
        raw_primary = _mapping(
            diagnostic["primary_span"],
            f"{context}.primary_span",
        )
        if "marker" in raw_primary:
            marker = _text(raw_primary["marker"], f"{context}.primary_span.marker")
            markers, _ = schema.source_primary_markers(test)
            locations = markers.get(marker, [])
            if len(locations) != 1:
                raise ValueError(
                    f"{context}.primary_span.marker must resolve exactly once"
                )
            source, line = locations[0]
            primary_span = model.MarkerPrimarySpan(
                marker=marker,
                source=source,
                line=line,
            )
        else:
            raw_bytes = _mapping(
                raw_primary["bytes"],
                f"{context}.primary_span.bytes",
            )
            primary_span = model.BytePrimarySpan(
                source=_text(raw_primary["source"], f"{context}.primary_span.source"),
                byte_range=diagnostic_model.ByteRange(
                    start=_integer(
                        raw_bytes["start"],
                        f"{context}.primary_span.bytes.start",
                    ),
                    end=_integer(
                        raw_bytes["end"],
                        f"{context}.primary_span.bytes.end",
                    ),
                ),
            )

    raw_data_equals = _mapping(
        diagnostic.get("data_equals", {}),
        f"{context}.data_equals",
    )
    return model.DiagnosticAssertion(
        code=_text(diagnostic["code"], f"{context}.code"),
        message_all_of=_string_tuple(
            diagnostic["message_contains"],
            f"{context}.message_contains",
        ),
        message_any_of=tuple(
            _string_tuple(group, f"{context}.message_contains_any[{index}]")
            for index, group in enumerate(
                _list(
                    diagnostic.get("message_contains_any", []),
                    f"{context}.message_contains_any",
                )
            )
        ),
        data_keys=_string_tuple(
            diagnostic["data_keys"],
            f"{context}.data_keys",
        ),
        data_equals=tuple(
            (
                key,
                diagnostic_contracts.expect_diagnostic_scalar(
                    raw_value,
                    f"{context}.data_equals.{key}",
                ),
            )
            for key, raw_value in sorted(raw_data_equals.items())
        ),
        help_contains=(
            None
            if "help_contains" not in diagnostic
            else _string_tuple(
                diagnostic["help_contains"],
                f"{context}.help_contains",
            )
        ),
        related_contains=(
            None
            if "related_contains" not in diagnostic
            else _string_tuple(
                diagnostic["related_contains"],
                f"{context}.related_contains",
            )
        ),
        primary_span=primary_span,
    )


def _decode_numeric_comparison(
    value: YamlValue,
    context: str,
) -> model.NumericComparison:
    numeric = _mapping(value, context)
    raw_expected = numeric["expected"]
    expected: model.NumericExpected
    if isinstance(raw_expected, list):
        expected = tuple(cast(model.NumericScalar, item) for item in raw_expected)
    else:
        expected = cast(model.NumericScalar, raw_expected)

    raw_shape = numeric.get("shape")
    shape = (
        None
        if raw_shape is None
        else tuple(cast(int, item) for item in _list(raw_shape, f"{context}.shape"))
    )
    raw_mode = numeric.get("mode")
    mode = (
        None
        if raw_mode is None
        else cast(
            model.NumericComparisonMode,
            _text(raw_mode, f"{context}.mode"),
        )
    )
    return model.NumericComparison(
        dtype=_text(numeric["dtype"], f"{context}.dtype"),
        expected=expected,
        shape=shape,
        mode=mode,
        relative_tolerance=cast(int | float | None, numeric.get("rtol")),
        absolute_tolerance=cast(int | float | None, numeric.get("atol")),
    )


def _decode_value(value: YamlValue, context: str) -> model.ValueOutcome:
    raw_value = _mapping(value, context)
    raw_numeric = raw_value.get("numeric_compare")
    return model.ValueOutcome(
        kind=_text(raw_value["kind"], f"{context}.kind"),
        representation=(
            None
            if "repr" not in raw_value
            else _text(raw_value["repr"], f"{context}.repr", allow_empty=True)
        ),
        error_variant=(
            None
            if "error_variant" not in raw_value
            else _text(raw_value["error_variant"], f"{context}.error_variant")
        ),
        numeric_comparison=(
            None
            if raw_numeric is None
            else _decode_numeric_comparison(
                raw_numeric,
                f"{context}.numeric_compare",
            )
        ),
    )


def _decode_test_reference(value: YamlValue, context: str) -> model.TestReference:
    reference = _mapping(value, context)
    return model.TestReference(
        module=_text(reference["module"], f"{context}.module"),
        name=_text(reference["name"], f"{context}.name"),
    )


def _decode_test_run(value: YamlValue, context: str) -> model.TestRunOutcome:
    test_run = _mapping(value, context)

    def references(field: str) -> tuple[model.TestReference, ...]:
        return tuple(
            _decode_test_reference(item, f"{context}.{field}[{index}]")
            for index, item in enumerate(_list(test_run[field], f"{context}.{field}"))
        )

    failed: list[model.TestFailure] = []
    for index, item in enumerate(_list(test_run["failed"], f"{context}.failed")):
        failure_context = f"{context}.failed[{index}]"
        raw_failure = _mapping(item, failure_context)
        failed.append(
            model.TestFailure(
                reference=_decode_test_reference(item, failure_context),
                kind=cast(
                    model.TestFailureKind,
                    _text(raw_failure["kind"], f"{failure_context}.kind"),
                ),
                message_contains=_string_tuple(
                    raw_failure["message_contains"],
                    f"{failure_context}.message_contains",
                ),
                runtime_code=(
                    None
                    if "runtime_code" not in raw_failure
                    else _text(
                        raw_failure["runtime_code"],
                        f"{failure_context}.runtime_code",
                    )
                ),
            )
        )
    return model.TestRunOutcome(
        order=references("order"),
        passed=references("passed"),
        skipped=references("skipped"),
        failed=tuple(failed),
    )


def _decode_expectation(
    value: YamlValue,
    test: dict[str, schema.YamlValue],
    context: str,
) -> model.Expectation:
    expectation = _mapping(value, context)
    if "compile_error" in expectation:
        outcome: model.ExpectedOutcome = model.CompileErrorOutcome(
            _decode_diagnostic(
                expectation["compile_error"],
                test,
                f"{context}.compile_error",
            )
        )
    elif "runtime_error" in expectation:
        outcome = model.RuntimeErrorOutcome(
            _decode_diagnostic(
                expectation["runtime_error"],
                test,
                f"{context}.runtime_error",
            )
        )
    elif "value" in expectation:
        outcome = _decode_value(expectation["value"], f"{context}.value")
    else:
        outcome = _decode_test_run(
            expectation["test_run"],
            f"{context}.test_run",
        )

    return model.Expectation(
        outcome=outcome,
        value_diagnostics=tuple(
            _decode_diagnostic(
                item,
                test,
                f"{context}.value_diagnostics[{index}]",
            )
            for index, item in enumerate(
                _list(
                    expectation.get("value_diagnostics", []),
                    f"{context}.value_diagnostics",
                )
            )
        ),
        diagnostic_contains=(
            None
            if "diagnostic_contains" not in expectation
            else _string_tuple(
                expectation["diagnostic_contains"],
                f"{context}.diagnostic_contains",
            )
        ),
        side_effects=_string_tuple(
            expectation.get("side_effects", []),
            f"{context}.side_effects",
        ),
        process_exit=cast(int | None, expectation.get("process_exit")),
    )


def _decode_test_case(
    raw_test: dict[str, schema.YamlValue],
    suite_file: str,
    index: int,
) -> model.TestCase:
    context = f"{suite_file}.tests[{index}]"
    invocation = decode_fixture_invocation(
        cast(Mapping[str, YamlValue], raw_test),
        context,
    )
    return model.TestCase(
        test_id=_text(raw_test["id"], f"{context}.id"),
        category=_text(raw_test["category"], f"{context}.category"),
        title=_text(raw_test["title"], f"{context}.title"),
        invocation=invocation,
        expectation=_decode_expectation(
            raw_test["expect"],
            raw_test,
            f"{context}.expect",
        ),
    )


def decode_suite_document(
    value: YamlValue,
    suite_file: str,
    channel_registry: diagnostic_model.DiagnosticRegistry | None = None,
) -> model.SuiteDocument:
    """Validate and decode one closed suite document."""
    root = _mapping(value, suite_file)
    unknown = sorted(set(root) - schema.SUITE_FILE_KEYS)
    if unknown:
        raise ValueError(f"{suite_file} has unsupported keys {unknown}")
    raw_tests = _list(root.get("tests"), f"{suite_file}.tests")
    if not raw_tests:
        raise ValueError(f"{suite_file}.tests must not be empty")

    validated: list[dict[str, schema.YamlValue]] = []
    test_ids: list[str] = []
    problems: list[ValidationProblem] = []
    for index, raw_test in enumerate(raw_tests):
        test_id, test_problems = schema.validate_test_case(
            raw_test,
            suite_file,
            index,
            channel_registry=channel_registry,
        )
        problems.extend(test_problems)
        if isinstance(raw_test, dict) and all(isinstance(key, str) for key in raw_test):
            validated.append(cast(dict[str, schema.YamlValue], raw_test))
        if test_id is not None:
            test_ids.append(test_id)

    duplicates = sorted(
        test_id for test_id, count in Counter(test_ids).items() if count > 1
    )
    problems.extend(
        ValidationProblem(f"{suite_file} has duplicate test id {test_id}")
        for test_id in duplicates
    )
    if problems:
        raise ValueError("; ".join(problem.message for problem in problems))
    return model.SuiteDocument(
        tests=tuple(
            _decode_test_case(test, suite_file, index)
            for index, test in enumerate(validated)
        )
    )


def decode_bundle(
    manifest_value: YamlValue,
    suite_documents: Mapping[str, YamlValue],
    context: str = "conformance_bundle",
    channel_registry: diagnostic_model.DiagnosticRegistry | None = None,
) -> model.ConformanceBundle:
    """Decode one manifest and its exact suite-document set."""
    manifest = schema.parse_manifest_value(
        manifest_value,
        f"{context}.manifest",
    )
    if not all(isinstance(path, str) for path in suite_documents):
        raise TypeError(f"{context}.suite_documents must use string keys")
    expected_files = {entry.file for entry in manifest.suites}
    supplied_files = set(suite_documents)
    if supplied_files != expected_files:
        missing = sorted(expected_files - supplied_files)
        unknown = sorted(supplied_files - expected_files)
        raise ValueError(
            f"{context}.suite_documents do not match manifest exactly: "
            f"missing={missing}, unknown={unknown}"
        )
    suites = tuple(
        model.ConformanceSuite(
            entry=entry,
            document=decode_suite_document(
                suite_documents[entry.file],
                entry.file,
                channel_registry=channel_registry,
            ),
        )
        for entry in manifest.suites
    )
    test_owners: dict[str, str] = {}
    for suite in suites:
        for test in suite.document.tests:
            existing_owner = test_owners.get(test.test_id)
            if existing_owner is not None:
                raise ValueError(
                    f"global duplicate test id {test.test_id}: "
                    f"{existing_owner}, {suite.entry.file}"
                )
            test_owners[test.test_id] = suite.entry.file
            if test.category == model.OPTIONAL_PROBE_CATEGORY and suite.entry.required:
                raise ValueError(
                    f"{suite.entry.file}/{test.test_id} uses "
                    f"{model.OPTIONAL_PROBE_CATEGORY!r} inside a required suite"
                )
    return model.ConformanceBundle(manifest=manifest, suites=suites)

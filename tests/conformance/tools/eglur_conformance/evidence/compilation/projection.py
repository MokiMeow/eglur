"""Derive compilation evidence projections from canonical suites."""

from dataclasses import dataclass

from eglur_conformance.artifact_content import JsonValue, canonical_digest
from eglur_conformance.diagnostics import contracts as diagnostic_contracts
from eglur_conformance.evidence.compilation import model as compile_model
from eglur_conformance.suite import model as suite_model
from eglur_conformance.yaml_document import YamlValue


@dataclass(frozen=True)
class ExpectedDiagnostic:
    """Portable compile-error assertions derived from one fixture.

    Parameters
    ----------
    code : str
        Required diagnostic code.
    message_all_of : tuple[str, ...]
        Message fragments that must all occur.
    message_any_of : tuple[tuple[str, ...], ...]
        Alternative message-fragment groups.
    data_keys : tuple[str, ...]
        Required structured-data keys.
    data_equals : tuple[tuple[str, compile_model.DiagnosticScalar], ...]
        Required structured-data values.
    primary_span : suite_model.PrimarySpanAssertion | None
        Marker-line or exact-byte assertion, when the fixture declares one.
    """

    code: str
    message_all_of: tuple[str, ...]
    message_any_of: tuple[tuple[str, ...], ...]
    data_keys: tuple[str, ...]
    data_equals: tuple[tuple[str, compile_model.DiagnosticScalar], ...]
    primary_span: suite_model.PrimarySpanAssertion | None


@dataclass(frozen=True)
class CompileFixture:
    """One manifest-owned fixture and its compile-phase expectation.

    Parameters
    ----------
    identity : suite_model.TestIdentity
        Manifest-qualified test identity.
    suite_file : str
        Owning suite-document path.
    precondition : str | None
        Conditional activation key.
    input_digest : str
        Canonical compile-input content address.
    expected_diagnostic : ExpectedDiagnostic | None
        Expected rejection diagnostic, or ``None`` for compile success.
    """

    identity: suite_model.TestIdentity
    suite_file: str
    precondition: str | None
    input_digest: str
    expected_diagnostic: ExpectedDiagnostic | None


@dataclass(frozen=True)
class ConformanceCompileInventory:
    """Compile fixtures derived from one exact manifest/suite document set.

    Parameters
    ----------
    release : str
        Manifest release label.
    fixtures : tuple[CompileFixture, ...]
        Required compile fixtures sorted by identity.
    """

    release: str
    fixtures: tuple[CompileFixture, ...]


def _registry_bytes_payload(
    value: suite_model.RegistryBytes,
    exact_key: str,
    repeat_key: str,
) -> dict[str, JsonValue]:
    if isinstance(value, bytes):
        return {exact_key: value.hex()}
    return {
        repeat_key: {
            "byte_hex": f"{value.byte:02x}",
            "count": value.count,
        }
    }


def _registry_entry_payload(entry: suite_model.RegistryEntry) -> dict[str, JsonValue]:
    payload: dict[str, JsonValue] = {"kind": "file"}
    if isinstance(entry, suite_model.RegistryFileEntry):
        payload.update(_registry_bytes_payload(entry.path, "path_hex", "path_repeat"))
        payload.update(
            _registry_bytes_payload(
                entry.content,
                "content_hex",
                "content_repeat",
            )
        )
        return payload

    if isinstance(entry, suite_model.RegistryDirectoryEntry):
        payload["kind"] = "directory"
    else:
        payload["kind"] = entry.kind
    payload.update(_registry_bytes_payload(entry.path, "path_hex", "path_repeat"))
    if isinstance(entry, suite_model.RegistryLinkEntry):
        payload.update(
            _registry_bytes_payload(entry.target, "target_hex", "target_repeat")
        )
    return payload


def _registry_provider_step_payload(
    step: suite_model.RegistryProviderStep,
) -> dict[str, JsonValue]:
    if isinstance(step, suite_model.RegistryEmitStep):
        return {"emit": _registry_entry_payload(step.entry)}
    if isinstance(step, suite_model.RegistryRepeatStep):
        return {
            "repeat": {
                "count": step.count,
                "emit": _registry_entry_payload(step.entry),
            }
        }
    return {"finish": step.outcome}


def _registry_acquisition_payload(
    acquisition: suite_model.RegistryAcquisition,
) -> dict[str, JsonValue]:
    return {
        "package": acquisition.package,
        "version": acquisition.version,
        "provider_steps": [
            _registry_provider_step_payload(step) for step in acquisition.provider_steps
        ],
    }


def compilation_input_payload(
    identity: suite_model.TestIdentity,
    invocation: suite_model.FixtureInvocation,
) -> dict[str, JsonValue]:
    """Return the exact source/invocation projection compiled by a runner.

    Parameters
    ----------
    identity : suite_model.TestIdentity
        Manifest-qualified test identity serialized into the payload.
    invocation : suite_model.FixtureInvocation
        Canonical source and runner invocation.

    Returns
    -------
    dict[str, JsonValue]
        Canonical digest input with the established serialized field shape.
    """
    base: dict[str, JsonValue] = {
        "suite_id": identity.suite_id,
        "test_id": identity.test_id,
        "mode": invocation.mode,
        "runner": invocation.runner,
        "registry_acquisitions": [
            _registry_acquisition_payload(acquisition)
            for acquisition in invocation.registry_acquisitions
        ],
    }
    source = invocation.source
    if isinstance(source, suite_model.Utf8Source):
        base["source"] = {
            "kind": "utf8",
            "text": source.text,
        }
        return base
    if isinstance(source, suite_model.RawBytesSource):
        base["source"] = {"kind": "bytes", "hex": source.hex_bytes}
        return base

    base["source"] = {
        "kind": "files",
        "files": {item.path: item.text for item in source.files},
        "symlinks": {item.path: item.target for item in source.symlinks},
        "entry": source.entry,
        "file_set_root": source.root,
    }
    return base


def compilation_input_digest(
    identity: suite_model.TestIdentity,
    invocation: suite_model.FixtureInvocation,
) -> str:
    """Address the exact source and compile-invocation projection.

    Parameters
    ----------
    identity : suite_model.TestIdentity
        Manifest-qualified test identity included in the address.
    invocation : suite_model.FixtureInvocation
        Canonical source and runner invocation.

    Returns
    -------
    str
        SHA-256 content address of the canonical invocation payload.
    """
    return canonical_digest(compilation_input_payload(identity, invocation))


def _diagnostic_scalar_from_yaml(
    value: YamlValue,
    context: str,
) -> compile_model.DiagnosticScalar:
    """Normalize a fixture scalar to its explicit evidence representation."""
    scalar = diagnostic_contracts.expect_diagnostic_scalar(value, context)
    return compile_model.DiagnosticScalar(
        kind=diagnostic_contracts.diagnostic_scalar_kind(scalar),
        value=scalar,
    )


def parse_expected_diagnostic(
    assertion: suite_model.DiagnosticAssertion,
) -> ExpectedDiagnostic:
    """Project a canonical diagnostic into compile-evidence assertions.

    Parameters
    ----------
    assertion : suite_model.DiagnosticAssertion
        Canonical suite diagnostic assertion.

    Returns
    -------
    ExpectedDiagnostic
        Portable compile-evidence expectation.

    Raises
    ------
    TypeError
        A structured diagnostic scalar has the wrong type.
    ValueError
        A structured diagnostic scalar is outside the closed scalar set.
    """
    data_equals = tuple(
        (
            key,
            _diagnostic_scalar_from_yaml(
                raw_scalar,
                f"diagnostic.data_equals.{key}",
            ),
        )
        for key, raw_scalar in assertion.data_equals
    )
    return ExpectedDiagnostic(
        code=assertion.code,
        message_all_of=assertion.message_all_of,
        message_any_of=assertion.message_any_of,
        data_keys=tuple(sorted(assertion.data_keys)),
        data_equals=data_equals,
        primary_span=assertion.primary_span,
    )


def conformance_compile_inventory(
    bundle: suite_model.ConformanceBundle,
) -> ConformanceCompileInventory:
    """Derive compile fixtures from one canonical conformance bundle.

    Parameters
    ----------
    bundle : suite_model.ConformanceBundle
        Canonical manifest and suite bundle.

    Returns
    -------
    ConformanceCompileInventory
        Required compile fixtures and their release identity.
    """
    fixtures: list[CompileFixture] = []
    for test in bundle.tests:
        if not test.suite.required:
            continue
        outcome = test.case.expectation.outcome
        expected_diagnostic = (
            parse_expected_diagnostic(outcome.diagnostic)
            if isinstance(outcome, suite_model.CompileErrorOutcome)
            else None
        )
        fixtures.append(
            CompileFixture(
                identity=test.identity,
                suite_file=test.suite.file,
                precondition=test.suite.precondition,
                input_digest=compilation_input_digest(
                    test.identity,
                    test.case.invocation,
                ),
                expected_diagnostic=expected_diagnostic,
            )
        )
    return ConformanceCompileInventory(
        release=bundle.manifest.release,
        fixtures=tuple(sorted(fixtures, key=lambda item: item.identity)),
    )

"""Project an authored selection from the canonical conformance bundle."""

from collections import defaultdict

from eglur_conformance.semantic_slice import model
from eglur_conformance.suite import model as suite_model


def project_corpus(
    bundle: suite_model.ConformanceBundle,
    selection: model.FixtureSelection,
    input_digest: str,
) -> model.SemanticFixtureCorpus:
    """Resolve every selected identity exactly once into generated fixtures.

    Parameters
    ----------
    bundle : suite_model.ConformanceBundle
        Canonical manifest-owned conformance suite.
    selection : model.FixtureSelection
        Authored identity-only fixture selection.
    input_digest : str
        Content address of the exact authoritative inputs.

    Returns
    -------
    model.SemanticFixtureCorpus
        Sorted generated source, invocation, and expectation projection.

    Raises
    ------
    TypeError
        An input has the wrong canonical model type.
    ValueError
        A selected identity does not resolve exactly once or is unsupported.
    """
    if not isinstance(bundle, suite_model.ConformanceBundle):
        raise TypeError("semantic fixture bundle must be a ConformanceBundle")
    if not isinstance(selection, model.FixtureSelection):
        raise TypeError("semantic fixture selection must be a FixtureSelection")

    tests_by_identity: dict[
        suite_model.TestIdentity,
        list[suite_model.ConformanceTest],
    ] = defaultdict(list)
    for test in bundle.tests:
        tests_by_identity[test.identity].append(test)

    fixtures: list[model.SemanticFixture] = []
    for identity in selection.identities:
        matches = tests_by_identity.get(identity, [])
        if len(matches) != 1:
            raise ValueError(
                f"fixture selection identity {identity} does not resolve exactly "
                f"once (found {len(matches)})"
            )
        selected = matches[0]
        fixtures.append(
            model.SemanticFixture(
                identity=identity,
                invocation=selected.case.invocation,
                expectation=selected.case.expectation,
            )
        )

    return model.SemanticFixtureCorpus(
        schema_version=model.SCHEMA_VERSION,
        input_digest=input_digest,
        fixtures=tuple(fixtures),
    )

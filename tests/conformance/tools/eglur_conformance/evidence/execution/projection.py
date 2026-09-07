"""Manifest-derived conformance fixture inventories."""

from collections.abc import Mapping
from dataclasses import dataclass

from eglur_conformance.suite import codec as suite_codec
from eglur_conformance.suite import model as suite_model
from eglur_conformance.yaml_document import YamlValue


@dataclass(frozen=True)
class ConformanceTestInventory:
    """Test identities derived from one manifest and its exact suite set.

    Parameters
    ----------
    known_test_identities : frozenset[suite_model.TestIdentity]
        All test identities in the canonical bundle.
    optional_probe_test_identities : frozenset[suite_model.TestIdentity]
        Test identities belonging to optional-probe suites.
    """

    known_test_identities: frozenset[suite_model.TestIdentity]
    optional_probe_test_identities: frozenset[suite_model.TestIdentity]


def parse_conformance_test_inventory(
    manifest_value: YamlValue,
    suite_documents: Mapping[str, YamlValue],
    context: str = "conformance_inventory",
) -> ConformanceTestInventory:
    """Decode suite artifacts and derive exact test identities.

    Parameters
    ----------
    manifest_value : YamlValue
        Parsed conformance manifest.
    suite_documents : Mapping[str, YamlValue]
        Parsed suite documents keyed by manifest path.
    context : str
        Diagnostic path used in validation errors.

    Returns
    -------
    ConformanceTestInventory
        Exact known and optional-probe identity sets.

    Raises
    ------
    TypeError
        A manifest or suite field has the wrong type.
    ValueError
        The manifest or suite set violates its canonical contract.
    """
    return conformance_test_inventory(
        suite_codec.decode_bundle(
            manifest_value,
            suite_documents,
            context,
        )
    )


def conformance_test_inventory(
    bundle: suite_model.ConformanceBundle,
) -> ConformanceTestInventory:
    """Derive test identities from one canonical conformance bundle.

    Parameters
    ----------
    bundle : suite_model.ConformanceBundle
        Canonical manifest and suite bundle.

    Returns
    -------
    ConformanceTestInventory
        Exact known and optional-probe identity sets.
    """
    known_test_identities: set[suite_model.TestIdentity] = set()
    optional_probe_test_identities: set[suite_model.TestIdentity] = set()
    for test in bundle.tests:
        known_test_identities.add(test.identity)
        if test.case.category == suite_model.OPTIONAL_PROBE_CATEGORY:
            optional_probe_test_identities.add(test.identity)

    return ConformanceTestInventory(
        known_test_identities=frozenset(known_test_identities),
        optional_probe_test_identities=frozenset(optional_probe_test_identities),
    )

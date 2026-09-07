"""Architecture tests for the production Rust compiler workspace."""

import tomllib
import unittest
from graphlib import CycleError, TopologicalSorter
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _TOOLS_DIR.parents[2]
_CRATES_ROOT = _REPO_ROOT / "compiler" / "crates"


def expected_production_graph() -> dict[str, frozenset[str]]:
    """Return the accepted internal dependency graph for production crates."""
    return {
        "eglur-source": frozenset(),
        "eglur-syntax": frozenset({"eglur-source"}),
        "eglur-hir": frozenset({"eglur-source", "eglur-syntax"}),
        "eglur-diagnostics": frozenset({"eglur-hir", "eglur-source"}),
        "eglur-sema": frozenset({"eglur-diagnostics", "eglur-hir", "eglur-source"}),
        "eglur-eval": frozenset({"eglur-hir", "eglur-sema"}),
        "eglur-driver": frozenset(
            {
                "eglur-diagnostics",
                "eglur-eval",
                "eglur-hir",
                "eglur-sema",
                "eglur-source",
                "eglur-syntax",
            }
        ),
    }


def read_toml(path: Path) -> dict[str, object]:
    """Decode one TOML object and reject non-string top-level keys."""
    raw: object = tomllib.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, dict) or not all(isinstance(key, str) for key in raw):
        raise ValueError(f"{path} is not a string-keyed TOML document")
    return {key: value for key, value in raw.items() if isinstance(key, str)}


def table(value: object, context: str) -> dict[str, object]:
    """Narrow one parsed TOML value to a string-keyed table."""
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} is not a string-keyed table")
    return {key: item for key, item in value.items() if isinstance(key, str)}


def string_list(value: object, context: str) -> tuple[str, ...]:
    """Narrow one parsed TOML value to a list of strings."""
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise ValueError(f"{context} is not a string list")
    return tuple(item for item in value if isinstance(item, str))


def package_manifest(package: str) -> tuple[Path, dict[str, object]]:
    """Return the manifest path and decoded document for one compiler package."""
    path = _CRATES_ROOT / package / "Cargo.toml"
    return path, read_toml(path)


def production_dependencies(
    package: str,
    manifest_path: Path,
    manifest: dict[str, object],
) -> frozenset[str]:
    """Return validated internal production dependencies from one manifest."""
    dependencies = table(manifest.get("dependencies", {}), f"{package}.dependencies")
    internal = {
        name: value for name, value in dependencies.items() if name.startswith("eglur-")
    }
    for name, value in internal.items():
        dependency = table(value, f"{package}.dependencies.{name}")
        path = dependency.get("path")
        if not isinstance(path, str):
            raise TypeError(f"{package} -> {name} lacks a local path")
        resolved = (manifest_path.parent / path).resolve()
        expected = (_CRATES_ROOT / name).resolve()
        if resolved != expected:
            raise ValueError(
                f"{package} -> {name} resolves to {resolved}, expected {expected}"
            )
    return frozenset(internal)


class CompilerPackageTopologyTests(unittest.TestCase):
    """Keep production compiler ownership and dependency direction explicit."""

    def test_workspace_members_match_the_owned_compiler_packages(self) -> None:
        expected_graph = expected_production_graph()
        manifest = read_toml(_REPO_ROOT / "Cargo.toml")
        workspace = table(manifest.get("workspace"), "workspace")
        members = string_list(workspace.get("members"), "workspace.members")
        expected = tuple(
            f"compiler/crates/{package}" for package in sorted(expected_graph)
        )

        self.assertEqual(tuple(sorted(members)), expected)

    def test_production_dependency_graph_matches_the_owned_layers(self) -> None:
        expected_graph = expected_production_graph()
        actual: dict[str, frozenset[str]] = {}
        for package in expected_graph:
            manifest_path, manifest = package_manifest(package)
            package_table = table(manifest.get("package"), f"{package}.package")
            self.assertEqual(package_table.get("name"), package)
            actual[package] = production_dependencies(
                package,
                manifest_path,
                manifest,
            )

        self.assertEqual(actual, expected_graph)

    def test_production_dependency_graph_is_acyclic(self) -> None:
        expected_graph = expected_production_graph()
        try:
            tuple(TopologicalSorter(expected_graph).static_order())
        except CycleError as error:
            cycle = " -> ".join(error.args[1])
            self.fail(f"compiler package dependency cycle: {cycle}")


if __name__ == "__main__":
    unittest.main()

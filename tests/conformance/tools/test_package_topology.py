"""Architecture tests for conformance tooling package dependencies."""

import ast
import unittest
from graphlib import CycleError, TopologicalSorter
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parent / "eglur_conformance"
EVIDENCE_ROOT = PACKAGE_ROOT / "evidence"
EXPECTED_EVIDENCE_MODULES = frozenset(
    {
        "compilation.codec",
        "compilation.model",
        "compilation.policy",
        "compilation.projection",
        "execution.codec",
        "execution.model",
        "execution.policy",
        "execution.projection",
        "execution_context.codec",
        "execution_context.model",
    }
)


def semantic_owner(source_file: Path) -> str:
    """Return the top-level module or package that owns one source file."""
    relative = source_file.relative_to(PACKAGE_ROOT)
    return relative.parts[0] if len(relative.parts) > 1 else source_file.stem


def imported_from_owners(
    source_file: Path,
    statement: ast.ImportFrom,
) -> set[str]:
    """Resolve first-party owners from one absolute or relative import."""
    if statement.level == 0:
        if statement.module is None:
            return set()
        parts = statement.module.split(".")
        if parts == ["eglur_conformance"]:
            return {alias.name.split(".", 1)[0] for alias in statement.names}
        if len(parts) > 1 and parts[0] == "eglur_conformance":
            return {parts[1]}
        return set()

    package_parts = source_file.relative_to(PACKAGE_ROOT).parent.parts
    parent_count = statement.level - 1
    if parent_count > len(package_parts):
        return set()
    base_parts = package_parts[: len(package_parts) - parent_count]
    module_parts = (
        () if statement.module is None else tuple(statement.module.split("."))
    )
    resolved_parts = (*base_parts, *module_parts)
    if resolved_parts:
        return {resolved_parts[0]}
    return {alias.name.split(".", 1)[0] for alias in statement.names}


def imported_owners(source_file: Path) -> set[str]:
    """Return first-party top-level owners imported by one source file."""
    tree = ast.parse(source_file.read_text(), filename=str(source_file))
    owners: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                parts = alias.name.split(".")
                if len(parts) > 1 and parts[0] == "eglur_conformance":
                    owners.add(parts[1])
        elif isinstance(node, ast.ImportFrom):
            owners.update(imported_from_owners(source_file, node))
    return owners


def package_dependencies() -> dict[str, set[str]]:
    """Build the semantic-owner dependency graph from Python imports."""
    source_files = sorted(PACKAGE_ROOT.rglob("*.py"))
    graph = {semantic_owner(source_file): set() for source_file in source_files}
    for source_file in source_files:
        owner = semantic_owner(source_file)
        graph[owner].update(
            dependency
            for dependency in imported_owners(source_file)
            if dependency in graph and dependency != owner
        )
    return graph


def evidence_module_name(source_file: Path) -> str:
    """Return one concrete module name relative to the evidence package."""
    return ".".join(source_file.relative_to(EVIDENCE_ROOT).with_suffix("").parts)


def evidence_modules() -> dict[str, Path]:
    """Return concrete evidence modules without package initializers."""
    return {
        evidence_module_name(source_file): source_file
        for source_file in EVIDENCE_ROOT.rglob("*.py")
        if source_file.name != "__init__.py"
    }


def imported_evidence_modules(
    source_file: Path,
    known_modules: set[str],
) -> set[str]:
    """Return concrete evidence modules imported by one source module."""
    tree = ast.parse(source_file.read_text(), filename=str(source_file))
    imported: set[str] = set()
    prefix = "eglur_conformance.evidence."
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            candidates = [alias.name for alias in node.names]
        elif isinstance(node, ast.ImportFrom):
            if node.level != 0:
                imported.add("<relative-import>")
                continue
            if node.module is None:
                continue
            candidates = [
                node.module,
                *(f"{node.module}.{alias.name}" for alias in node.names),
            ]
        else:
            continue
        for candidate in candidates:
            if not candidate.startswith(prefix):
                continue
            relative = candidate.removeprefix(prefix)
            matches = [
                module
                for module in known_modules
                if relative == module or relative.startswith(f"{module}.")
            ]
            if matches:
                imported.add(max(matches, key=len))
    return imported


class PackageTopologyTests(unittest.TestCase):
    """Keep stable primitives below domain packages and release composition above."""

    def test_semantic_package_graph_is_acyclic(self) -> None:
        graph = package_dependencies()

        try:
            tuple(TopologicalSorter(graph).static_order())
        except CycleError as error:
            cycle = " -> ".join(error.args[1])
            self.fail(f"conformance tooling package dependency cycle: {cycle}")

    def test_lower_owners_do_not_import_spec_release(self) -> None:
        graph = package_dependencies()
        importers = sorted(
            owner
            for owner, dependencies in graph.items()
            if owner != "spec_release" and "spec_release" in dependencies
        )

        self.assertEqual(importers, [])

    def test_relative_import_cannot_bypass_owner_resolution(self) -> None:
        tree = ast.parse("from ..spec_release import gate")
        statement = tree.body[0]
        self.assertIsInstance(statement, ast.ImportFrom)

        owners = imported_from_owners(
            PACKAGE_ROOT / "suite" / "synthetic.py",
            statement,
        )

        self.assertEqual(owners, {"spec_release"})

    def test_evidence_modules_have_one_semantic_role(self) -> None:
        self.assertEqual(set(evidence_modules()), EXPECTED_EVIDENCE_MODULES)

    def test_evidence_module_dependencies_point_inward(self) -> None:
        modules = evidence_modules()
        known_modules = set(modules)
        for module, source_file in modules.items():
            parts = module.split(".")
            if len(parts) != 2:
                continue
            domain, role = parts
            dependencies = imported_evidence_modules(source_file, known_modules)
            same_domain = {
                f"{domain}.model",
                f"{domain}.codec",
                f"{domain}.projection",
            }
            if role == "model":
                allowed: set[str] = set()
            elif role in {"codec", "projection"}:
                allowed = {f"{domain}.model"}
            else:
                allowed = same_domain
                if domain == "execution_context":
                    allowed = {"execution_context.model"}
                else:
                    allowed |= {
                        "execution_context.codec",
                        "execution_context.model",
                    }
            self.assertEqual(
                dependencies - allowed,
                set(),
                f"{module} has outward evidence dependencies",
            )

    def test_evidence_package_initializers_are_empty(self) -> None:
        for source_file in EVIDENCE_ROOT.rglob("__init__.py"):
            with self.subTest(source_file=source_file):
                tree = ast.parse(source_file.read_text(), filename=str(source_file))
                self.assertEqual(tree.body, [])


if __name__ == "__main__":
    unittest.main()

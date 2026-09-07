"""Regression tests for trusted standard-library effect metadata."""

import copy
import tempfile
import unittest
from pathlib import Path
from typing import ClassVar, cast

from eglur_conformance.effects import codec, render, source_catalog
from eglur_conformance.effects.model import (
    CallbackKind,
    EffectCatalog,
    EffectCategory,
    KnownEffectSummary,
    ResolvedCallableEffect,
    ResolvedEffectCatalog,
    StateScope,
    UnknownEffectSummary,
)
from eglur_conformance.yaml_document import YamlValue, load_yaml

_TOOLS_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _TOOLS_DIR.parents[2]
_CATALOG_PATH = _REPO_ROOT / codec.DEFAULT_CATALOG_PATH


def _mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        raise TypeError(f"{context} must be a string-keyed mapping")
    return cast(dict[str, YamlValue], value)


def _list(value: YamlValue, context: str) -> list[YamlValue]:
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


class EffectSummaryCatalogTests(unittest.TestCase):
    """Exercise source alignment and high-risk effect distinctions."""

    source: ClassVar[dict[str, YamlValue]]
    authored: ClassVar[EffectCatalog]
    catalog: ClassVar[ResolvedEffectCatalog]

    @classmethod
    def setUpClass(cls) -> None:
        """Load the canonical source once for mutation-based checks."""
        cls.source = _mapping(load_yaml(_CATALOG_PATH), str(_CATALOG_PATH))
        cls.authored = codec.decode_effect_catalog(
            cast(YamlValue, cls.source),
            str(_CATALOG_PATH),
        )
        cls.catalog = codec.load_effect_catalog(_REPO_ROOT)

    def _find(self, module: str, declaration_prefix: str) -> ResolvedCallableEffect:
        matches = tuple(
            item
            for item in self.catalog.callables
            if item.source.key.module == module
            and item.source.key.declaration.startswith(declaration_prefix)
        )
        self.assertEqual(len(matches), 1)
        return matches[0]

    def _resolve(self, source: dict[str, YamlValue]) -> None:
        decoded = codec.decode_effect_catalog(
            cast(YamlValue, source),
            "mutated effect catalog",
        )
        extracted = source_catalog.extract_source_catalog(
            _REPO_ROOT,
            decoded.baseline_index,
        )
        codec.resolve_effect_catalog(extracted, decoded)

    def test_resolves_complete_baseline_surface(self) -> None:
        self.assertEqual(len(self.catalog.callables), 399)
        self.assertEqual(len(self.catalog.aliases), 2)
        self.assertEqual(len(self.authored.callables), 162)
        self.assertEqual(
            tuple(alias.source.key.name for alias in self.catalog.aliases),
            ("concat", "format"),
        )

    def test_extracts_only_normative_baseline_modules(self) -> None:
        extracted = source_catalog.extract_source_catalog(
            _REPO_ROOT,
            "docs/spec/STDLIB.md",
        )

        self.assertEqual(len(extracted.sources), 17)
        self.assertTrue(
            all(path.startswith("docs/spec/stdlib/") for path in extracted.sources)
        )
        self.assertNotIn("std::prob", {item.key.module for item in extracted.callables})

    def test_source_identity_includes_next_line_requires_clause(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            stdlib_dir = repo_root / "docs/spec/stdlib"
            stdlib_dir.mkdir(parents=True)
            (repo_root / "docs/spec/STDLIB.md").write_text(
                """### Baseline Modules (v0.1)

1. [`std::demo` (REQUIRED)](stdlib/demo.md)

### Preview Modules (non-baseline)
""",
                encoding="utf-8",
            )
            (stdlib_dir / "demo.md").write_text(
                """# `std::demo`

```eglur-catalog
fn choose[T](value: T) -> Bool
  requires Eq[T]
fn plain() -> Unit
```
""",
                encoding="utf-8",
            )

            extracted = source_catalog.extract_source_catalog(
                repo_root,
                "docs/spec/STDLIB.md",
            )

        self.assertEqual(
            extracted.callables[0].key.declaration,
            "fn choose[T](value: T) -> Bool requires Eq[T]",
        )
        self.assertEqual(extracted.callables[1].key.declaration, "fn plain() -> Unit")

    def test_source_extraction_fails_closed_on_unparsed_callable_layout(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repo_root = Path(directory)
            stdlib_dir = repo_root / "docs/spec/stdlib"
            stdlib_dir.mkdir(parents=True)
            (repo_root / "docs/spec/STDLIB.md").write_text(
                """### Baseline Modules (v0.1)

1. [`std::demo` (REQUIRED)](stdlib/demo.md)

### Preview Modules (non-baseline)
""",
                encoding="utf-8",
            )
            (stdlib_dir / "demo.md").write_text(
                """# `std::demo`

```eglur-catalog
fn first() -> Unit fn second() -> Unit
```
""",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "unparsed callable declaration"):
                source_catalog.extract_source_catalog(
                    repo_root,
                    "docs/spec/STDLIB.md",
                )

    def test_preserves_representative_effect_distinctions(self) -> None:
        print_effect = self._find("std::io", "fn print!")
        now_effect = self._find("std::time", "fn now!")
        push_effect = self._find("std::vec", "fn Vec[T].push!")
        read_effect = self._find(
            "std::io",
            "fn Own[ByteReader].read_chunk!",
        )
        snapshot_effect = self._find("std::io", "fn process_snapshot!")
        spawn_effect = self._find("std::concurrency", "fn spawn!")
        abort_effect = self._find("std::test", "fn abort!")

        self.assertEqual(
            print_effect.own_summary,
            KnownEffectSummary((EffectCategory.IO,), StateScope.NONE),
        )
        self.assertEqual(
            now_effect.own_summary,
            KnownEffectSummary((EffectCategory.NONDET,), StateScope.NONE),
        )
        self.assertEqual(
            push_effect.own_summary,
            KnownEffectSummary((EffectCategory.STATE,), StateScope.LOCALIZABLE),
        )
        self.assertEqual(
            read_effect.own_summary,
            KnownEffectSummary(
                (EffectCategory.IO, EffectCategory.STATE),
                StateScope.LOCALIZABLE,
            ),
        )
        self.assertEqual(
            snapshot_effect.own_summary,
            KnownEffectSummary(
                (EffectCategory.IO, EffectCategory.NONDET),
                StateScope.NONE,
            ),
        )
        self.assertEqual(
            spawn_effect.own_summary,
            KnownEffectSummary(
                (EffectCategory.STATE, EffectCategory.NONDET),
                StateScope.NONLOCAL,
            ),
        )
        self.assertEqual(spawn_effect.callbacks[0].kind, CallbackKind.TASK_THUNK)
        self.assertIsInstance(abort_effect.own_summary, UnknownEffectSummary)

    def test_separates_higher_order_own_and_callback_effects(self) -> None:
        pure_map = self._find("std::seq", "fn Seq[T].map[U]")
        effectful_map = self._find("std::seq", "fn Seq[T].map![U]")
        retain = self._find("std::vec", "fn Vec[T].retain!")

        self.assertTrue(cast(KnownEffectSummary, pure_map.own_summary).is_pure)
        self.assertEqual(pure_map.callbacks[0].kind, CallbackKind.PURE)
        self.assertTrue(cast(KnownEffectSummary, effectful_map.own_summary).is_pure)
        self.assertEqual(effectful_map.callbacks[0].kind, CallbackKind.EFFECTFUL)
        self.assertEqual(
            retain.own_summary,
            KnownEffectSummary((EffectCategory.STATE,), StateScope.LOCALIZABLE),
        )
        self.assertEqual(retain.callbacks[0].kind, CallbackKind.EFFECTFUL)

    def test_every_non_bang_callable_has_known_pure_own_summary(self) -> None:
        failures = [
            item.source.key.render()
            for item in self.catalog.callables
            if not item.source.bang
            and (
                not isinstance(item.own_summary, KnownEffectSummary)
                or not item.own_summary.is_pure
            )
        ]
        self.assertEqual(failures, [])

    def test_rejects_illegal_known_state_scope(self) -> None:
        source = copy.deepcopy(self.source)
        profiles = _mapping(source["profiles"], "profiles")
        pure = _mapping(profiles["pure"], "profiles.pure")
        pure["state_scope"] = "localizable"

        with self.assertRaisesRegex(ValueError, "exactly when STATE is absent"):
            codec.decode_effect_catalog(
                cast(YamlValue, source),
                "mutated effect catalog",
            )

    def test_rejects_alternate_baseline_index_path(self) -> None:
        source = copy.deepcopy(self.source)
        source["baseline_index"] = "../STDLIB.md"

        with self.assertRaisesRegex(ValueError, "baseline_index must be"):
            codec.decode_effect_catalog(
                cast(YamlValue, source),
                "mutated effect catalog",
            )

    def test_rejects_missing_bang_or_callback_rule(self) -> None:
        source = copy.deepcopy(self.source)
        modules = _mapping(source["callables"], "callables")
        seq_rules = _list(modules["std::seq"], "callables.std::seq")
        seq_rules.pop(0)

        with self.assertRaisesRegex(ValueError, "requires an explicit"):
            self._resolve(source)

    def test_rejects_callback_metadata_drift(self) -> None:
        source = copy.deepcopy(self.source)
        modules = _mapping(source["callables"], "callables")
        seq_rules = _list(modules["std::seq"], "callables.std::seq")
        first_rule = _mapping(seq_rules[0], "callables.std::seq[0]")
        callbacks = _mapping(first_rule["callbacks"], "callbacks")
        callbacks["f"] = "effectful"

        with self.assertRaisesRegex(ValueError, "callback metadata mismatch"):
            self._resolve(source)

    def test_rejects_unknown_callable_rule(self) -> None:
        source = copy.deepcopy(self.source)
        modules = _mapping(source["callables"], "callables")
        seq_rules = _list(modules["std::seq"], "callables.std::seq")
        first_rule = _mapping(seq_rules[0], "callables.std::seq[0]")
        first_rule["declaration"] = "fn Seq[T].missing!() -> Unit"

        with self.assertRaisesRegex(ValueError, "unknown callables"):
            self._resolve(source)

    def test_rejects_preview_source_leakage(self) -> None:
        source = copy.deepcopy(self.source)
        sources = _list(source["sources"], "sources")
        sources.append("docs/roadmap/stdlib/prob.md")

        with self.assertRaisesRegex(ValueError, "do not match the baseline"):
            self._resolve(source)

    def test_rejects_unused_summary_profile(self) -> None:
        source = copy.deepcopy(self.source)
        profiles = _mapping(source["profiles"], "profiles")
        profiles["unused"] = {
            "kind": "Known",
            "categories": [],
            "state_scope": "none",
        }

        with self.assertRaisesRegex(ValueError, "unused profiles"):
            codec.decode_effect_catalog(
                cast(YamlValue, source),
                "mutated effect catalog",
            )

    def test_rejects_alias_target_drift(self) -> None:
        source = copy.deepcopy(self.source)
        aliases = _list(source["aliases"], "aliases")
        first_alias = _mapping(aliases[0], "aliases[0]")
        first_alias["target"] = "std::str::missing"

        with self.assertRaisesRegex(ValueError, "aliases do not match"):
            self._resolve(source)

    def test_reference_markdown_is_a_catalog_projection(self) -> None:
        markdown = render.render_markdown(self.catalog)

        self.assertTrue(markdown.startswith("# Standard Library Effect Summaries\n"))
        self.assertIn("## `std::concurrency`", markdown)
        self.assertIn("`Known({STATE, NONDET}, nonlocal)`", markdown)
        self.assertIn("`std::prelude::concat`", markdown)


if __name__ == "__main__":
    unittest.main()

"""Decode and resolve the normative stdlib effect-summary catalog."""

import re
from pathlib import Path
from typing import Final, cast

from eglur_conformance.effects.model import (
    AliasKey,
    CallableEffectRule,
    CallableKey,
    CallbackContract,
    CallbackKind,
    EffectCatalog,
    EffectCategory,
    EffectSummary,
    KnownEffectSummary,
    ResolvedAliasEffect,
    ResolvedCallableEffect,
    ResolvedEffectCatalog,
    SourceAlias,
    SourceCallable,
    SourceCatalog,
    StateScope,
    UnknownEffectSummary,
)
from eglur_conformance.effects.source_catalog import extract_source_catalog
from eglur_conformance.yaml_document import YamlValue, load_yaml

DEFAULT_CATALOG_PATH: Final = Path("docs/spec/stdlib/effect-summaries.yaml")
_BASELINE_INDEX_PATH = "docs/spec/STDLIB.md"
_SUPPORTED_VERSION = "0.1"
_PROFILE_NAME = re.compile(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")
_MODULE_NAME = re.compile(r"^std(?:::[A-Za-z_][A-Za-z0-9_]*)+$")
_CALLABLE_PATH = re.compile(
    r"^std(?:::[A-Za-z_][A-Za-z0-9_]*)+::[A-Za-z_][A-Za-z0-9_]*!?$"
)
_ALIAS_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*!?$")
_TRAIT_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
_CATEGORY_ORDER = (
    EffectCategory.IO,
    EffectCategory.STATE,
    EffectCategory.NONDET,
)
_ROOT_KEYS = frozenset(
    {
        "version",
        "baseline_index",
        "sources",
        "profiles",
        "default_own_summary",
        "callables",
        "aliases",
    }
)


def _expect_mapping(value: YamlValue, context: str) -> dict[str, YamlValue]:
    if not isinstance(value, dict):
        raise TypeError(f"{context} must be a mapping")
    if not all(isinstance(key, str) for key in value):
        raise ValueError(f"{context} must use string keys")
    return cast(dict[str, YamlValue], value)


def _expect_list(value: YamlValue, context: str) -> list[YamlValue]:
    if not isinstance(value, list):
        raise TypeError(f"{context} must be a list")
    return value


def _expect_str(value: YamlValue, context: str) -> str:
    if not isinstance(value, str):
        raise TypeError(f"{context} must be a string")
    if not value:
        raise ValueError(f"{context} must be non-empty")
    return value


def _expect_exact_keys(
    mapping: dict[str, YamlValue],
    expected: set[str],
    context: str,
) -> None:
    if set(mapping) != expected:
        raise ValueError(
            f"{context} keys must be {sorted(expected)}, found {sorted(mapping)}"
        )


def _expect_profile_name(value: YamlValue, context: str) -> str:
    name = _expect_str(value, context)
    if _PROFILE_NAME.fullmatch(name) is None:
        raise ValueError(f"{context} must be a lowercase kebab-case profile name")
    return name


def _decode_summary(value: YamlValue, context: str) -> EffectSummary:
    mapping = _expect_mapping(value, context)
    kind = _expect_str(mapping.get("kind"), f"{context}.kind")
    if kind == "Unknown":
        _expect_exact_keys(mapping, {"kind"}, context)
        return UnknownEffectSummary()
    if kind != "Known":
        raise ValueError(f"{context}.kind must be 'Known' or 'Unknown'")

    _expect_exact_keys(mapping, {"kind", "categories", "state_scope"}, context)
    try:
        categories = tuple(
            EffectCategory(_expect_str(item, f"{context}.categories[{index}]"))
            for index, item in enumerate(
                _expect_list(mapping["categories"], f"{context}.categories")
            )
        )
    except ValueError as error:
        raise ValueError(
            f"{context}.categories may contain only IO, STATE, and NONDET"
        ) from error
    if len(categories) != len(set(categories)):
        raise ValueError(f"{context}.categories must not contain duplicates")
    expected_order = tuple(
        category for category in _CATEGORY_ORDER if category in categories
    )
    if categories != expected_order:
        raise ValueError(
            f"{context}.categories must use canonical IO, STATE, NONDET order"
        )
    try:
        state_scope = StateScope(
            _expect_str(mapping["state_scope"], f"{context}.state_scope")
        )
    except ValueError as error:
        raise ValueError(
            f"{context}.state_scope must be none, localizable, or nonlocal"
        ) from error
    has_state = EffectCategory.STATE in categories
    if has_state == (state_scope is StateScope.NONE):
        raise ValueError(
            f"{context} must use state_scope=none exactly when STATE is absent"
        )
    return KnownEffectSummary(categories=categories, state_scope=state_scope)


def _decode_callbacks(value: YamlValue, context: str) -> tuple[CallbackContract, ...]:
    mapping = _expect_mapping(value, context)
    callbacks: list[CallbackContract] = []
    for parameter, raw_kind in mapping.items():
        if _TRAIT_NAME.fullmatch(parameter) is None:
            raise ValueError(f"{context} has invalid callback parameter {parameter!r}")
        try:
            kind = CallbackKind(_expect_str(raw_kind, f"{context}.{parameter}"))
        except ValueError as error:
            raise ValueError(
                f"{context}.{parameter} has unsupported callback kind"
            ) from error
        callbacks.append(CallbackContract(parameter=parameter, kind=kind))
    return tuple(callbacks)


def _decode_callable_rule(
    value: YamlValue,
    module: str,
    index: int,
) -> CallableEffectRule:
    context = f"effect catalog.callables.{module}[{index}]"
    mapping = _expect_mapping(value, context)
    allowed_keys = {"trait", "declaration", "own_summary", "callbacks"}
    required_keys = allowed_keys - {"trait"}
    if not required_keys <= set(mapping) or not set(mapping) <= allowed_keys:
        raise ValueError(
            f"{context} requires {sorted(required_keys)} and permits optional trait"
        )
    trait = (
        _expect_str(mapping["trait"], f"{context}.trait") if "trait" in mapping else ""
    )
    if trait and _TRAIT_NAME.fullmatch(trait) is None:
        raise ValueError(f"{context}.trait must be an identifier")
    declaration = _expect_str(mapping["declaration"], f"{context}.declaration")
    if declaration != re.sub(r"\s+", " ", declaration).strip():
        raise ValueError(f"{context}.declaration must use normalized whitespace")
    return CallableEffectRule(
        key=CallableKey(module=module, trait=trait, declaration=declaration),
        own_summary_profile=_expect_profile_name(
            mapping["own_summary"],
            f"{context}.own_summary",
        ),
        callbacks=_decode_callbacks(mapping["callbacks"], f"{context}.callbacks"),
    )


def decode_effect_catalog(document: YamlValue, context: str) -> EffectCatalog:
    """Decode one strict stdlib effect-summary catalog document.

    Parameters
    ----------
    document
        Parsed YAML document.
    context
        Human-readable source name for validation errors.

    Returns
    -------
    EffectCatalog
        Typed authored catalog before source-declaration resolution.
    """
    root = _expect_mapping(document, context)
    _expect_exact_keys(root, set(_ROOT_KEYS), context)
    version = _expect_str(root["version"], f"{context}.version")
    if version != _SUPPORTED_VERSION:
        raise ValueError(
            f"{context}.version must be {_SUPPORTED_VERSION!r}, found {version!r}"
        )
    baseline_index = _expect_str(
        root["baseline_index"],
        f"{context}.baseline_index",
    )
    if baseline_index != _BASELINE_INDEX_PATH:
        raise ValueError(f"{context}.baseline_index must be {_BASELINE_INDEX_PATH!r}")
    sources = tuple(
        _expect_str(item, f"{context}.sources[{index}]")
        for index, item in enumerate(
            _expect_list(root["sources"], f"{context}.sources")
        )
    )
    if len(sources) != len(set(sources)):
        raise ValueError(f"{context}.sources must not contain duplicates")

    raw_profiles = _expect_mapping(root["profiles"], f"{context}.profiles")
    profiles = tuple(
        (
            _expect_profile_name(name, f"{context}.profiles key"),
            _decode_summary(value, f"{context}.profiles.{name}"),
        )
        for name, value in raw_profiles.items()
    )
    if not profiles:
        raise ValueError(f"{context}.profiles must be non-empty")
    default_profile = _expect_profile_name(
        root["default_own_summary"],
        f"{context}.default_own_summary",
    )
    profile_names = {name for name, _ in profiles}
    if default_profile not in profile_names:
        raise ValueError(f"{context}.default_own_summary references unknown profile")

    raw_modules = _expect_mapping(root["callables"], f"{context}.callables")
    rules: list[CallableEffectRule] = []
    for module, raw_rules in raw_modules.items():
        if _MODULE_NAME.fullmatch(module) is None:
            raise ValueError(f"{context}.callables has invalid module {module!r}")
        module_rules = _expect_list(raw_rules, f"{context}.callables.{module}")
        if not module_rules:
            raise ValueError(f"{context}.callables.{module} must be non-empty")
        rules.extend(
            _decode_callable_rule(raw_rule, module, index)
            for index, raw_rule in enumerate(module_rules)
        )
    rule_keys = [rule.key for rule in rules]
    if len(rule_keys) != len(set(rule_keys)):
        raise ValueError(f"{context}.callables contains duplicate source identities")
    unknown_profiles = sorted(
        {rule.own_summary_profile for rule in rules} - profile_names
    )
    if unknown_profiles:
        raise ValueError(
            f"{context}.callables references unknown profiles {unknown_profiles}"
        )

    aliases: list[AliasKey] = []
    for index, raw_alias in enumerate(
        _expect_list(root["aliases"], f"{context}.aliases")
    ):
        alias_context = f"{context}.aliases[{index}]"
        alias = _expect_mapping(raw_alias, alias_context)
        _expect_exact_keys(alias, {"module", "name", "target"}, alias_context)
        module = _expect_str(alias["module"], f"{alias_context}.module")
        name = _expect_str(alias["name"], f"{alias_context}.name")
        target = _expect_str(alias["target"], f"{alias_context}.target")
        if _MODULE_NAME.fullmatch(module) is None:
            raise ValueError(f"{alias_context}.module is invalid")
        if _ALIAS_NAME.fullmatch(name) is None:
            raise ValueError(f"{alias_context}.name is invalid")
        if _CALLABLE_PATH.fullmatch(target) is None:
            raise ValueError(f"{alias_context}.target is invalid")
        aliases.append(AliasKey(module=module, name=name, target=target))
    if len(aliases) != len(set(aliases)):
        raise ValueError(f"{context}.aliases must not contain duplicates")

    used_profiles = {default_profile, *(rule.own_summary_profile for rule in rules)}
    unused_profiles = sorted(profile_names - used_profiles)
    if unused_profiles:
        raise ValueError(
            f"{context}.profiles contains unused profiles {unused_profiles}"
        )

    return EffectCatalog(
        version=version,
        baseline_index=baseline_index,
        sources=sources,
        profiles=profiles,
        default_own_summary_profile=default_profile,
        callables=tuple(rules),
        aliases=tuple(aliases),
    )


def _target_name(source: SourceCallable) -> str | None:
    declaration = source.key.declaration
    matched = re.match(r"^(?:(?:optional|overload)\s+)*fn\s+", declaration)
    if matched is None:
        return None
    head = declaration[matched.end() :]
    generic_depth = 0
    call_start = None
    for index, character in enumerate(head):
        if character == "[":
            generic_depth += 1
        elif character == "]":
            generic_depth -= 1
        elif character == "(" and generic_depth == 0:
            call_start = index
            break
    if call_start is None:
        return None
    callable_head = head[:call_start].split("[", 1)[0]
    return callable_head if _ALIAS_NAME.fullmatch(callable_head) is not None else None


def _resolve_alias(
    alias: SourceAlias,
    resolved_callables: tuple[ResolvedCallableEffect, ...],
) -> ResolvedAliasEffect:
    target_module, separator, target_name = alias.key.target.rpartition("::")
    if not separator:
        raise ValueError(
            f"{alias.source_path}:{alias.source_line}: invalid alias target"
        )
    targets = tuple(
        callable_effect
        for callable_effect in resolved_callables
        if callable_effect.source.key.module == target_module
        and callable_effect.source.key.trait == ""
        and _target_name(callable_effect.source) == target_name
    )
    if not targets:
        raise ValueError(
            f"{alias.source_path}:{alias.source_line}: alias target "
            f"{alias.key.target} does not resolve"
        )
    target_metadata = {(target.own_summary, target.callbacks) for target in targets}
    if len(target_metadata) != 1:
        raise ValueError(
            f"{alias.source_path}:{alias.source_line}: alias target overloads "
            "have heterogeneous effect metadata"
        )
    return ResolvedAliasEffect(source=alias, targets=targets)


def resolve_effect_catalog(
    source_catalog: SourceCatalog,
    catalog: EffectCatalog,
) -> ResolvedEffectCatalog:
    """Resolve authored metadata against the normalized stdlib callable surface.

    Parameters
    ----------
    source_catalog
        Callable identities extracted from authoritative baseline module docs.
    catalog
        Decoded authored effect metadata.

    Returns
    -------
    ResolvedEffectCatalog
        Total metadata for every extracted callable and alias.
    """
    if catalog.sources != source_catalog.sources:
        raise ValueError(
            "effect catalog sources do not match the baseline module index: "
            f"catalog={catalog.sources}, index={source_catalog.sources}"
        )
    default_summary = catalog.profile(catalog.default_own_summary_profile)
    if (
        not isinstance(default_summary, KnownEffectSummary)
        or not default_summary.is_pure
    ):
        raise ValueError("default_own_summary must resolve to the known pure summary")

    source_by_key = {source.key: source for source in source_catalog.callables}
    rule_by_key = {rule.key: rule for rule in catalog.callables}
    extra_rules = sorted(set(rule_by_key) - set(source_by_key))
    if extra_rules:
        raise ValueError(
            "effect catalog contains rules for unknown callables: "
            f"{[key.render() for key in extra_rules]}"
        )

    resolved: list[ResolvedCallableEffect] = []
    for source in source_catalog.callables:
        rule = rule_by_key.get(source.key)
        requires_explicit_rule = source.bang or bool(source.callbacks)
        if rule is None:
            if requires_explicit_rule:
                raise ValueError(
                    f"{source.location()}: {source.key.render()} requires an explicit "
                    "effect catalog rule"
                )
            resolved.append(
                ResolvedCallableEffect(
                    source=source,
                    own_summary=default_summary,
                    callbacks=(),
                )
            )
            continue

        if not requires_explicit_rule:
            raise ValueError(
                f"{source.location()}: {source.key.render()} has a redundant explicit rule"
            )
        if rule.callbacks != source.callbacks:
            raise ValueError(
                f"{source.location()}: callback metadata mismatch for "
                f"{source.key.render()}: catalog={rule.callbacks}, "
                f"signature={source.callbacks}"
            )
        own_summary = catalog.profile(rule.own_summary_profile)
        if not source.bang and (
            not isinstance(own_summary, KnownEffectSummary) or not own_summary.is_pure
        ):
            raise ValueError(
                f"{source.location()}: non-bang callable must have known pure own summary"
            )
        resolved.append(
            ResolvedCallableEffect(
                source=source,
                own_summary=own_summary,
                callbacks=rule.callbacks,
            )
        )

    source_alias_keys = tuple(alias.key for alias in source_catalog.aliases)
    if catalog.aliases != source_alias_keys:
        raise ValueError(
            "effect catalog aliases do not match stdlib re-exports: "
            f"catalog={catalog.aliases}, source={source_alias_keys}"
        )
    resolved_callables = tuple(resolved)
    resolved_aliases = tuple(
        _resolve_alias(alias, resolved_callables) for alias in source_catalog.aliases
    )
    return ResolvedEffectCatalog(
        version=catalog.version,
        callables=resolved_callables,
        aliases=resolved_aliases,
    )


def load_effect_catalog(
    repo_root: Path,
    catalog_path: Path = DEFAULT_CATALOG_PATH,
) -> ResolvedEffectCatalog:
    """Load and resolve the repository's trusted stdlib effect catalog.

    Parameters
    ----------
    repo_root
        Repository root containing the specification package.
    catalog_path
        Absolute path or repository-relative path to the authored YAML catalog.

    Returns
    -------
    ResolvedEffectCatalog
        Complete source-aligned effect metadata.
    """
    path = catalog_path if catalog_path.is_absolute() else repo_root / catalog_path
    catalog = decode_effect_catalog(load_yaml(path), str(path))
    source_catalog = extract_source_catalog(repo_root, catalog.baseline_index)
    return resolve_effect_catalog(source_catalog, catalog)

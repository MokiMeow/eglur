"""Render human-readable reference material from resolved effect metadata."""

from eglur_conformance.effects.model import (
    CallbackContract,
    ResolvedEffectCatalog,
)


def _escape_table_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def _render_callbacks(callbacks: tuple[CallbackContract, ...]) -> str:
    if not callbacks:
        return "none"
    return ", ".join(
        f"`{callback.parameter}`: `{callback.kind.value}`" for callback in callbacks
    )


def render_markdown(catalog: ResolvedEffectCatalog) -> str:
    """Render the resolved catalog as a deterministic Markdown reference.

    Parameters
    ----------
    catalog
        Fully validated effect catalog.

    Returns
    -------
    str
        Markdown grouped by fully qualified stdlib module.
    """
    lines = [
        "# Standard Library Effect Summaries",
        "",
        (
            "Generated from `docs/spec/stdlib/effect-summaries.yaml` and the "
            "normative `eglur-catalog` signatures."
        ),
        "",
    ]
    modules = sorted(
        {callable_effect.source.key.module for callable_effect in catalog.callables}
    )
    for module in modules:
        lines.extend(
            [
                f"## `{module}`",
                "",
                "| Declaration | Own summary | Callback contract |",
                "| --- | --- | --- |",
            ]
        )
        module_callables = sorted(
            (
                callable_effect
                for callable_effect in catalog.callables
                if callable_effect.source.key.module == module
            ),
            key=lambda item: (
                item.source.key.trait,
                item.source.key.declaration,
            ),
        )
        for callable_effect in module_callables:
            declaration = callable_effect.source.key.declaration
            if callable_effect.source.key.trait:
                declaration = f"[{callable_effect.source.key.trait}] {declaration}"
            lines.append(
                "| `"
                + _escape_table_cell(declaration)
                + "` | `"
                + callable_effect.own_summary.render()
                + "` | "
                + _render_callbacks(callable_effect.callbacks)
                + " |"
            )
        lines.append("")

    if catalog.aliases:
        lines.extend(
            [
                "## Callable Re-exports",
                "",
                "| Alias | Target | Target overloads |",
                "| --- | --- | ---: |",
            ]
        )
        for alias in sorted(catalog.aliases, key=lambda item: item.source.key):
            lines.append(
                f"| `{alias.source.key.module}::{alias.source.key.name}` | "
                f"`{alias.source.key.target}` | {len(alias.targets)} |"
            )
        lines.append("")
    return "\n".join(lines)

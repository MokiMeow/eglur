"""Typed models for standard-library callable effect metadata."""

from dataclasses import dataclass
from enum import StrEnum


class EffectCategory(StrEnum):
    """One category in the closed v0.1 effect-summary lattice."""

    IO = "IO"
    STATE = "STATE"
    NONDET = "NONDET"


class StateScope(StrEnum):
    """How summarized state mutation is rooted at a call boundary."""

    NONE = "none"
    LOCALIZABLE = "localizable"
    NONLOCAL = "nonlocal"


class CallbackKind(StrEnum):
    """One callback contract admitted by the stdlib catalog metalanguage."""

    PURE = "pure"
    EFFECTFUL = "effectful"
    SCOPED_RESOURCE = "scoped-resource"
    TASK_THUNK = "task-thunk"


@dataclass(frozen=True)
class KnownEffectSummary:
    """One sound finite-lattice summary."""

    categories: tuple[EffectCategory, ...]
    state_scope: StateScope

    @property
    def is_pure(self) -> bool:
        """Return whether the summary has no observable effect category."""
        return not self.categories and self.state_scope is StateScope.NONE

    def render(self) -> str:
        """Render the normative summary notation."""
        categories = ", ".join(category.value for category in self.categories)
        return f"Known({{{categories}}}, {self.state_scope.value})"


@dataclass(frozen=True)
class UnknownEffectSummary:
    """The conservative top value when no sound known summary exists."""

    def render(self) -> str:
        """Render the normative summary notation."""
        return "Unknown"


EffectSummary = KnownEffectSummary | UnknownEffectSummary


@dataclass(frozen=True, order=True)
class CallableKey:
    """Source identity for one normalized catalog callable declaration."""

    module: str
    trait: str
    declaration: str

    def render(self) -> str:
        """Render a stable human-readable source identity."""
        context = f" [{self.trait}]" if self.trait else ""
        return f"{self.module}{context}: {self.declaration}"


@dataclass(frozen=True)
class CallbackContract:
    """One named callback parameter and its effect contract."""

    parameter: str
    kind: CallbackKind


@dataclass(frozen=True)
class SourceCallable:
    """One callable extracted from an authoritative stdlib catalog block."""

    key: CallableKey
    callbacks: tuple[CallbackContract, ...]
    bang: bool
    source_path: str
    source_line: int

    def location(self) -> str:
        """Return the declaration's source location."""
        return f"{self.source_path}:{self.source_line}"


@dataclass(frozen=True, order=True)
class AliasKey:
    """Visible identity of one callable re-export."""

    module: str
    name: str
    target: str


@dataclass(frozen=True)
class SourceAlias:
    """One callable re-export extracted from a stdlib catalog block."""

    key: AliasKey
    source_path: str
    source_line: int


@dataclass(frozen=True)
class SourceCatalog:
    """Callable declarations and aliases extracted from baseline module docs."""

    sources: tuple[str, ...]
    callables: tuple[SourceCallable, ...]
    aliases: tuple[SourceAlias, ...]


@dataclass(frozen=True)
class CallableEffectRule:
    """Explicit trusted metadata for one source callable."""

    key: CallableKey
    own_summary_profile: str
    callbacks: tuple[CallbackContract, ...]


@dataclass(frozen=True)
class EffectCatalog:
    """Decoded authored effect-summary catalog before source resolution."""

    version: str
    baseline_index: str
    sources: tuple[str, ...]
    profiles: tuple[tuple[str, EffectSummary], ...]
    default_own_summary_profile: str
    callables: tuple[CallableEffectRule, ...]
    aliases: tuple[AliasKey, ...]

    def profile(self, name: str) -> EffectSummary:
        """Return one named summary profile."""
        for profile_name, summary in self.profiles:
            if profile_name == name:
                return summary
        raise KeyError(name)


@dataclass(frozen=True)
class ResolvedCallableEffect:
    """Total trusted effect metadata for one source callable."""

    source: SourceCallable
    own_summary: EffectSummary
    callbacks: tuple[CallbackContract, ...]


@dataclass(frozen=True)
class ResolvedAliasEffect:
    """One re-export bound to target callable metadata identities."""

    source: SourceAlias
    targets: tuple[ResolvedCallableEffect, ...]


@dataclass(frozen=True)
class ResolvedEffectCatalog:
    """Complete source-aligned effect metadata for the baseline stdlib."""

    version: str
    callables: tuple[ResolvedCallableEffect, ...]
    aliases: tuple[ResolvedAliasEffect, ...]

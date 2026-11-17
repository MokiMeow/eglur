use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ExternalDefId;

mod authorization;
mod body;
mod inference;

pub use inference::{EffectAnalysisError, EffectFacts, EffectScc, analyze_effects};

/// One category in the closed v0.1 effect-summary lattice.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EffectCategory {
    Io,
    State,
    Nondet,
}

/// Reachability of state mutation summarized for one callable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StateScope {
    None,
    Localizable,
    Nonlocal,
}

impl StateScope {
    #[must_use]
    pub(crate) const fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Nonlocal, _) | (_, Self::Nonlocal) => Self::Nonlocal,
            (Self::Localizable, _) | (_, Self::Localizable) => Self::Localizable,
            (Self::None, Self::None) => Self::None,
        }
    }
}

/// One validated known point in the v0.1 effect-summary lattice.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KnownEffectSummary {
    categories: Box<[EffectCategory]>,
    state_scope: StateScope,
}

impl KnownEffectSummary {
    pub fn new(
        categories: impl IntoIterator<Item = EffectCategory>,
        state_scope: StateScope,
    ) -> Result<Self, EffectSummaryError> {
        let categories = categories.into_iter().collect::<BTreeSet<_>>();
        let has_state = categories.contains(&EffectCategory::State);
        if has_state != !matches!(state_scope, StateScope::None) {
            return Err(EffectSummaryError::InvalidStateScope {
                has_state,
                state_scope,
            });
        }
        Ok(Self {
            categories: categories
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            state_scope,
        })
    }

    #[must_use]
    pub fn pure() -> Self {
        Self {
            categories: Box::new([]),
            state_scope: StateScope::None,
        }
    }

    #[must_use]
    pub fn categories(&self) -> &[EffectCategory] {
        &self.categories
    }

    #[must_use]
    pub const fn state_scope(&self) -> StateScope {
        self.state_scope
    }

    #[must_use]
    pub fn is_pure(&self) -> bool {
        self.categories.is_empty()
    }

    #[must_use]
    pub(crate) fn contains(&self, category: EffectCategory) -> bool {
        self.categories.binary_search(&category).is_ok()
    }

    #[must_use]
    pub(crate) fn join(&self, other: &Self) -> Self {
        let categories = self
            .categories
            .iter()
            .chain(other.categories.iter())
            .copied()
            .collect::<BTreeSet<_>>();
        Self {
            categories: categories
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            state_scope: self.state_scope.join(other.state_scope),
        }
    }

    #[must_use]
    pub(crate) fn without_state(&self) -> Self {
        Self {
            categories: self
                .categories
                .iter()
                .copied()
                .filter(|category| *category != EffectCategory::State)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            state_scope: StateScope::None,
        }
    }

    #[must_use]
    pub(crate) fn is_localizable_state_only(&self) -> bool {
        self.categories.as_ref() == [EffectCategory::State]
            && self.state_scope == StateScope::Localizable
    }
}

/// Compile-time effect fact for one resolved callable.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EffectSummary {
    Unknown,
    Known(KnownEffectSummary),
}

impl EffectSummary {
    #[must_use]
    pub fn pure() -> Self {
        Self::Known(KnownEffectSummary::pure())
    }

    #[must_use]
    pub(crate) fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Known(left), Self::Known(right)) => Self::Known(left.join(right)),
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
        }
    }
}

/// One trusted external callable effect fact before graph binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedEffectSummary {
    declaration: ExternalDefId,
    summary: EffectSummary,
}

impl TrustedEffectSummary {
    #[must_use]
    pub const fn new(declaration: ExternalDefId, summary: EffectSummary) -> Self {
        Self {
            declaration,
            summary,
        }
    }

    #[must_use]
    pub const fn declaration(&self) -> &ExternalDefId {
        &self.declaration
    }

    #[must_use]
    pub const fn summary(&self) -> &EffectSummary {
        &self.summary
    }
}

/// Deterministically ordered trusted effect facts keyed by external identity.
pub struct TrustedEffectEnvironment {
    summaries: Box<[TrustedEffectSummary]>,
    by_id: BTreeMap<ExternalDefId, usize>,
}

impl TrustedEffectEnvironment {
    pub fn new(
        summaries: impl IntoIterator<Item = TrustedEffectSummary>,
    ) -> Result<Self, TrustedEffectError> {
        let mut summaries = summaries.into_iter().collect::<Vec<_>>();
        summaries.sort_by(|left, right| left.declaration.cmp(&right.declaration));
        let mut by_id = BTreeMap::new();
        for (index, summary) in summaries.iter().enumerate() {
            if by_id.insert(summary.declaration.clone(), index).is_some() {
                return Err(TrustedEffectError::DuplicateDeclaration(
                    summary.declaration.clone(),
                ));
            }
        }
        Ok(Self {
            summaries: summaries.into_boxed_slice(),
            by_id,
        })
    }

    #[must_use]
    pub fn summary(&self, declaration: &ExternalDefId) -> Option<&EffectSummary> {
        self.by_id
            .get(declaration)
            .map(|&index| self.summaries[index].summary())
    }

    pub fn summaries(&self) -> impl ExactSizeIterator<Item = &TrustedEffectSummary> {
        self.summaries.iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectSummaryError {
    InvalidStateScope {
        has_state: bool,
        state_scope: StateScope,
    },
}

impl fmt::Display for EffectSummaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStateScope {
                has_state,
                state_scope,
            } => write!(
                formatter,
                "effect summary state membership ({has_state}) conflicts with scope {state_scope:?}"
            ),
        }
    }
}

impl std::error::Error for EffectSummaryError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedEffectError {
    DuplicateDeclaration(ExternalDefId),
}

impl fmt::Display for TrustedEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateDeclaration(declaration) => write!(
                formatter,
                "duplicate trusted effect summary {}",
                declaration.as_str()
            ),
        }
    }
}

impl std::error::Error for TrustedEffectError {}

#[cfg(test)]
mod tests {
    use super::{
        EffectCategory, EffectSummary, EffectSummaryError, KnownEffectSummary, StateScope,
        TrustedEffectEnvironment, TrustedEffectError, TrustedEffectSummary,
    };
    use crate::ExternalDefId;

    #[test]
    fn known_summary_normalizes_categories_and_rejects_invalid_state_scope() {
        let summary = KnownEffectSummary::new(
            [
                EffectCategory::State,
                EffectCategory::Io,
                EffectCategory::State,
            ],
            StateScope::Localizable,
        )
        .unwrap();
        assert_eq!(
            summary.categories(),
            [EffectCategory::Io, EffectCategory::State]
        );
        assert!(matches!(
            KnownEffectSummary::new([EffectCategory::Io], StateScope::Nonlocal),
            Err(EffectSummaryError::InvalidStateScope { .. })
        ));

        let io = KnownEffectSummary::new([EffectCategory::Io], StateScope::None).unwrap();
        let joined = summary.join(&io);
        assert_eq!(
            joined.categories(),
            [EffectCategory::Io, EffectCategory::State]
        );
        assert_eq!(joined.state_scope(), StateScope::Localizable);
        assert_eq!(joined.without_state().categories(), [EffectCategory::Io]);
        assert!(!summary.is_localizable_state_only());
    }

    #[test]
    fn unknown_is_the_summary_join_top() {
        let pure = EffectSummary::pure();
        let unknown = EffectSummary::Unknown;

        assert_eq!(pure.join(&unknown), EffectSummary::Unknown);
        assert_eq!(unknown.join(&pure), EffectSummary::Unknown);
        assert_eq!(pure.join(&pure), pure);
    }

    #[test]
    fn trusted_effect_environment_is_ordered_and_rejects_duplicates() {
        let first = ExternalDefId::new("std::test::abort!").unwrap();
        let second = ExternalDefId::new("std::test::emit!").unwrap();
        let environment = TrustedEffectEnvironment::new([
            TrustedEffectSummary::new(second.clone(), EffectSummary::pure()),
            TrustedEffectSummary::new(first.clone(), EffectSummary::Unknown),
        ])
        .unwrap();
        assert_eq!(
            environment
                .summaries()
                .map(|summary| summary.declaration().as_str())
                .collect::<Vec<_>>(),
            [first.as_str(), second.as_str()]
        );
        assert!(matches!(
            TrustedEffectEnvironment::new([
                TrustedEffectSummary::new(first.clone(), EffectSummary::Unknown),
                TrustedEffectSummary::new(first.clone(), EffectSummary::pure()),
            ]),
            Err(TrustedEffectError::DuplicateDeclaration(id)) if id == first
        ));
    }
}

use std::collections::BTreeMap;
use std::fmt;

use eglur_diagnostics::Poison;

/// Stored semantic result after an operation was actually performed.
#[derive(Debug)]
pub enum SemanticFact<T> {
    Known(T),
    Poisoned(Poison),
    NotApplicable,
}

/// Explicit query result that keeps unperformed operations distinct.
#[derive(Clone, Copy, Debug)]
pub enum FactState<'fact, T> {
    Unperformed,
    Known(&'fact T),
    Poisoned(&'fact Poison),
    NotApplicable,
}

/// Phase-owned fact table with single-assignment keys.
#[derive(Debug)]
pub(crate) struct FactTable<K, T> {
    facts: BTreeMap<K, SemanticFact<T>>,
}

impl<K: Ord, T> FactTable<K, T> {
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            facts: BTreeMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, key: K, fact: SemanticFact<T>) -> Result<(), DuplicateFact> {
        match self.facts.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(fact);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => Err(DuplicateFact),
        }
    }

    #[must_use]
    pub(crate) fn state(&self, key: &K) -> FactState<'_, T> {
        self.recorded_state(key).unwrap_or(FactState::Unperformed)
    }

    #[must_use]
    pub(crate) fn recorded_state(&self, key: &K) -> Option<FactState<'_, T>> {
        self.facts.get(key).map(|fact| match fact {
            SemanticFact::Known(value) => FactState::Known(value),
            SemanticFact::Poisoned(poison) => FactState::Poisoned(poison),
            SemanticFact::NotApplicable => FactState::NotApplicable,
        })
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = (&K, &SemanticFact<T>)> {
        self.facts.iter()
    }

    #[must_use]
    pub(crate) fn has_poisoned(&self) -> bool {
        self.facts
            .values()
            .any(|fact| matches!(fact, SemanticFact::Poisoned(_)))
    }

    #[must_use]
    pub(crate) fn all_known(&self) -> bool {
        self.facts
            .values()
            .all(|fact| matches!(fact, SemanticFact::Known(_)))
    }
}

impl<K: Ord, T> Default for FactTable<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DuplicateFact;

impl fmt::Display for DuplicateFact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("semantic fact key was assigned more than once")
    }
}

impl std::error::Error for DuplicateFact {}

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;
use std::sync::Arc;

use crate::root::RootIdentity;
use crate::{Diagnostic, DiagnosticCode, DiagnosticRoot, DiagnosticSnapshot, ErrorLimit, Severity};

#[derive(Debug)]
struct StoreIdentity;

/// Non-empty opaque provenance for facts unavailable after retained roots.
#[derive(Clone)]
pub struct Poison {
    store: Arc<StoreIdentity>,
    roots: Box<[usize]>,
}

impl Poison {
    fn single(store: Arc<StoreIdentity>, root: usize) -> Self {
        Self {
            store,
            roots: Box::new([root]),
        }
    }

    /// Merges two provenance sets from the same diagnostic store.
    pub fn merge(&self, other: &Self) -> Result<Self, DiagnosticStoreError> {
        if !Arc::ptr_eq(&self.store, &other.store) {
            return Err(DiagnosticStoreError::ForeignPoison);
        }

        let mut roots = Vec::with_capacity(self.roots.len() + other.roots.len());
        let (mut left, mut right) = (0, 0);
        while left < self.roots.len() && right < other.roots.len() {
            match self.roots[left].cmp(&other.roots[right]) {
                std::cmp::Ordering::Less => {
                    roots.push(self.roots[left]);
                    left += 1;
                }
                std::cmp::Ordering::Greater => {
                    roots.push(other.roots[right]);
                    right += 1;
                }
                std::cmp::Ordering::Equal => {
                    roots.push(self.roots[left]);
                    left += 1;
                    right += 1;
                }
            }
        }
        roots.extend_from_slice(&self.roots[left..]);
        roots.extend_from_slice(&other.roots[right..]);

        Ok(Self {
            store: Arc::clone(&self.store),
            roots: roots.into_boxed_slice(),
        })
    }
}

impl fmt::Debug for Poison {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Poison")
            .field("root_count", &self.roots.len())
            .finish_non_exhaustive()
    }
}

/// Phase-owned classification of one failed semantic check.
#[derive(Debug)]
pub enum DiagnosticFailure {
    /// An independently established failed obligation.
    Root(DiagnosticRoot),
    /// A check that cannot decide without already-poisoned prerequisites.
    Derivative(Poison),
}

/// Internal contract failure while retaining diagnostics or poison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticStoreError {
    /// The same root identity was rediscovered with a different record.
    ConflictingRoot(DiagnosticCode),
    /// Poison from another store was used as local provenance.
    ForeignPoison,
    /// An error attempted to bypass independent-root classification.
    ErrorObservation,
}

impl fmt::Display for DiagnosticStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingRoot(code) => write!(
                formatter,
                "diagnostic root {code} was rediscovered with conflicting record data",
            ),
            Self::ForeignPoison => {
                formatter.write_str("poison provenance belongs to another diagnostic store")
            }
            Self::ErrorObservation => {
                formatter.write_str("error diagnostics must be recorded as independent roots")
            }
        }
    }
}

impl std::error::Error for DiagnosticStoreError {}

#[derive(Debug)]
struct StoredRoot {
    id: usize,
    diagnostic: Diagnostic,
}

/// Single mutable authority for diagnostic roots and poisoned provenance.
pub struct DiagnosticStore {
    identity: Arc<StoreIdentity>,
    roots: HashMap<RootIdentity, StoredRoot>,
    observations: Vec<Diagnostic>,
    next_root: usize,
}

impl DiagnosticStore {
    /// Creates an empty independent diagnostic lifecycle.
    #[must_use]
    pub fn new() -> Self {
        Self {
            identity: Arc::new(StoreIdentity),
            roots: HashMap::new(),
            observations: Vec::new(),
            next_root: 0,
        }
    }

    /// Records a root or propagates a derivative without emitting another one.
    pub fn record_failure(
        &mut self,
        failure: DiagnosticFailure,
    ) -> Result<Poison, DiagnosticStoreError> {
        match failure {
            DiagnosticFailure::Root(root) => self.record_root(root),
            DiagnosticFailure::Derivative(poison) => {
                self.validate_poison(&poison)?;
                Ok(poison)
            }
        }
    }

    /// Retains a warning or note that is not part of root/poison semantics.
    pub fn record_observation(
        &mut self,
        diagnostic: Diagnostic,
    ) -> Result<(), DiagnosticStoreError> {
        if diagnostic.severity() == Severity::Error {
            return Err(DiagnosticStoreError::ErrorObservation);
        }
        self.observations.push(diagnostic);
        Ok(())
    }

    /// Reports whether at least one independently established error root exists.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.roots.is_empty()
    }

    /// Consumes discovery state and freezes one canonically ordered snapshot.
    #[must_use]
    pub fn finalize(self, limit: Option<ErrorLimit>) -> DiagnosticSnapshot {
        let errors = self
            .roots
            .into_values()
            .map(|root| root.diagnostic)
            .collect();
        DiagnosticSnapshot::finalize(errors, self.observations, limit)
    }

    fn record_root(&mut self, root: DiagnosticRoot) -> Result<Poison, DiagnosticStoreError> {
        let (identity, diagnostic) = root.into_parts();
        let next_root = self.next_root;
        let root_id = match self.roots.entry(identity) {
            Entry::Occupied(entry) => {
                let established = entry.get();
                if established.diagnostic != diagnostic {
                    return Err(DiagnosticStoreError::ConflictingRoot(
                        diagnostic.code().clone(),
                    ));
                }
                established.id
            }
            Entry::Vacant(entry) => {
                self.next_root = self
                    .next_root
                    .checked_add(1)
                    .expect("diagnostic root count exceeds platform capacity");
                entry.insert(StoredRoot {
                    id: next_root,
                    diagnostic,
                });
                next_root
            }
        };
        Ok(Poison::single(Arc::clone(&self.identity), root_id))
    }

    fn validate_poison(&self, poison: &Poison) -> Result<(), DiagnosticStoreError> {
        if Arc::ptr_eq(&self.identity, &poison.store) {
            Ok(())
        } else {
            Err(DiagnosticStoreError::ForeignPoison)
        }
    }
}

impl Default for DiagnosticStore {
    fn default() -> Self {
        Self::new()
    }
}

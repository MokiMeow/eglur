use super::*;
use std::rc::Rc;

mod graph;
mod inventory;
#[cfg(test)]
mod tests;

pub(super) type ConstructionId = usize;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ConstructionKey {
    Bound(DefId, TypeRefId),
    Alias(DefId),
    Cohort(usize),
}

#[derive(Clone)]
pub(super) enum ConstructedReference {
    Bound(TraitRef),
    Alias(AliasTemplate),
    Cohort(ReferenceCohort),
}

#[derive(Clone)]
pub(super) enum ReferenceCohort {
    Unique(TraitRef),
    Multiple,
}

#[derive(Clone)]
pub(super) enum ConstructionState {
    Pending,
    Active,
    Blocked(Option<Poison>),
    Complete(Resolution<ConstructedReference>),
}

#[derive(Clone, Copy)]
pub(super) struct ProjectionSite {
    pub(super) file: FileId,
    pub(super) origin: OriginId,
}

struct Dependency {
    target: ConstructionId,
    projection: Option<ProjectionSite>,
}

struct ConstructionNode {
    key: ConstructionKey,
    state: ConstructionState,
    dependencies: Vec<Dependency>,
}

#[derive(Clone)]
pub(super) struct DeclaringBound {
    pub(super) cohort: ConstructionId,
    pub(super) associated: CanonicalTarget,
}

#[derive(Clone)]
pub(super) enum DeclaringSelection {
    Unique(DeclaringBound),
    Missing,
    Ambiguous,
}

#[derive(Default)]
pub(super) struct SourceBoundConstruction {
    pub(super) initialized: bool,
    nodes: Vec<ConstructionNode>,
    by_key: BTreeMap<ConstructionKey, ConstructionId>,
    heads: BTreeMap<DefId, BTreeMap<CanonicalTarget, ConstructionId>>,
    pub(super) cohorts: Vec<Rc<[ConstructionId]>>,
    associated_heads: BTreeMap<Box<str>, Vec<Rc<CanonicalTarget>>>,
    pub(super) head_results: BTreeMap<(FileId, TypeRefId), Resolution<CanonicalTarget>>,
    head_states: BTreeMap<DefId, Resolution<()>>,
    candidates: BTreeMap<(DefId, Box<str>), Resolution<DeclaringSelection>>,
    pub(super) environments: BTreeMap<DefId, Resolution<()>>,
    alias_parameters: BTreeMap<DefId, Rc<[(usize, GenericParameterId)]>>,
    #[cfg(test)]
    pub(super) work: ConstructionWork,
}

#[cfg(test)]
#[derive(Default, Debug)]
pub(super) struct ConstructionWork {
    pub(super) head_probes: usize,
    pub(super) bound_starts: usize,
    pub(super) cohort_starts: usize,
}

impl SourceBoundConstruction {
    pub(super) fn node(&mut self, key: ConstructionKey) -> ConstructionId {
        if let Some(&id) = self.by_key.get(&key) {
            return id;
        }
        let id = self.nodes.len();
        self.nodes.push(ConstructionNode {
            key,
            state: ConstructionState::Pending,
            dependencies: Vec::new(),
        });
        self.by_key.insert(key, id);
        id
    }

    pub(super) fn key(&self, id: ConstructionId) -> ConstructionKey {
        self.nodes[id].key
    }

    pub(super) fn state(&self, id: ConstructionId) -> &ConstructionState {
        &self.nodes[id].state
    }

    pub(super) fn set_state(&mut self, id: ConstructionId, state: ConstructionState) {
        self.nodes[id].state = state;
    }

    pub(super) fn depend(
        &mut self,
        caller: ConstructionId,
        target: ConstructionId,
        projection: Option<ProjectionSite>,
    ) {
        self.nodes[caller]
            .dependencies
            .push(Dependency { target, projection });
    }
}

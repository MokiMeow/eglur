use super::{AssociatedBindingSignature, cycles};
use crate::types::PayloadDependency;
use crate::{CanonicalTarget, CanonicalTypeStore, TraitRef, TypeId, TypeStoreError};
use std::collections::{BTreeMap, BTreeSet};

enum DependencyNode {
    Binding(usize),
    Payload(TypeId),
}

enum DependencyStep {
    RootValidated,
    PayloadInspected,
    PayloadEdgeScheduled,
    ProjectionLinked,
    EdgeIndexed,
}

pub(super) struct BindingDependencies {
    bindings: Vec<CanonicalTarget>,
    edges: Vec<Vec<usize>>,
}

pub(super) struct AssociatedBindingCycles {
    pub(super) cyclic: BTreeSet<CanonicalTarget>,
    pub(super) affected: BTreeSet<CanonicalTarget>,
}

impl BindingDependencies {
    pub(super) fn new(
        types: &CanonicalTypeStore,
        trait_ref: &TraitRef,
        target: TypeId,
        bindings: &[AssociatedBindingSignature],
    ) -> Result<Self, TypeStoreError> {
        Self::new_observed(types, trait_ref, target, bindings, |_| {})
    }

    fn new_observed(
        types: &CanonicalTypeStore,
        trait_ref: &TraitRef,
        target: TypeId,
        bindings: &[AssociatedBindingSignature],
        mut observe: impl FnMut(DependencyStep),
    ) -> Result<Self, TypeStoreError> {
        let nodes = bindings
            .iter()
            .map(|binding| binding.associated.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let binding_indices = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node, index))
            .collect::<BTreeMap<_, _>>();

        let mut payloads = BTreeMap::new();
        for binding in bindings {
            observe(DependencyStep::RootValidated);
            types.kind(binding.value)?;
            let mut pending = vec![binding.value];
            while let Some(current) = pending.pop() {
                if payloads.contains_key(&current) {
                    continue;
                }
                observe(DependencyStep::PayloadInspected);
                let mut dependencies = Vec::new();
                for dependency in types.payload_dependencies(current)? {
                    match dependency {
                        PayloadDependency::Type(child) => {
                            observe(DependencyStep::PayloadEdgeScheduled);
                            dependencies.push(DependencyNode::Payload(child));
                            pending.push(child);
                        }
                        PayloadDependency::Projection(projection)
                            if projection.base() == target
                                && projection.trait_ref() == trait_ref =>
                        {
                            if let Some(&index) = binding_indices.get(projection.associated()) {
                                observe(DependencyStep::ProjectionLinked);
                                dependencies.push(DependencyNode::Binding(index));
                            }
                        }
                        PayloadDependency::Projection(_) => {}
                    }
                }
                payloads.insert(current, dependencies);
            }
        }

        // Binding vertices are the prefix projected by cycle_sets. Payload vertices
        // preserve sharing instead of expanding a projection inventory for each root.
        let offset = nodes.len();
        let payload_indices = payloads
            .keys()
            .enumerate()
            .map(|(index, &id)| (id, offset + index))
            .collect::<BTreeMap<_, _>>();
        let mut edges = vec![Vec::new(); offset + payloads.len()];
        for binding in bindings {
            observe(DependencyStep::EdgeIndexed);
            edges[binding_indices[&binding.associated]].push(payload_indices[&binding.value]);
        }
        for (index, dependencies) in payloads.values().enumerate() {
            for dependency in dependencies {
                observe(DependencyStep::EdgeIndexed);
                let next = match dependency {
                    DependencyNode::Binding(index) => *index,
                    DependencyNode::Payload(id) => payload_indices[id],
                };
                edges[offset + index].push(next);
            }
        }
        Ok(Self {
            bindings: nodes,
            edges,
        })
    }

    pub(super) fn cycle_sets(&self) -> Option<AssociatedBindingCycles> {
        // Supported type constructors admit only earlier, immutable operands. Thus
        // every joint cycle contains a binding, and binding paths contract exactly.
        let statuses = cycles::classify(&self.edges);
        let cyclic = self
            .bindings
            .iter()
            .zip(&statuses)
            .filter(|(_, status)| status.is_cyclic())
            .map(|(node, _)| node.clone())
            .collect::<BTreeSet<_>>();
        if cyclic.is_empty() {
            return None;
        }
        let affected = self
            .bindings
            .iter()
            .zip(statuses)
            .filter(|(_, status)| status.is_affected())
            .map(|(node, _)| node.clone())
            .collect();
        Some(AssociatedBindingCycles { cyclic, affected })
    }
}

#[cfg(test)]
mod tests;

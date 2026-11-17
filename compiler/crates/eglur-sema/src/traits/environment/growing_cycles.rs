use std::collections::{HashMap, HashSet};

use crate::{DeclarationSignature, IntrinsicNominal, NominalHead};

use super::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CycleId(usize);

struct WrapperEdge {
    required: CanonicalTarget,
    arity: usize,
    grows: bool,
}

pub(super) struct GrowingCycles {
    groups: HashMap<CanonicalTarget, CycleId>,
    #[cfg(test)]
    pub(super) wrapper_work: (usize, usize),
}

#[derive(Default)]
pub(super) struct CycleBlockers {
    subjects: HashSet<(CycleId, TypeId)>,
}

impl GrowingCycles {
    fn collect(
        implementations: &BTreeMap<CanonicalTarget, TraitImpl>,
        signatures: &DeclarationSignatures,
    ) -> Result<Self, TraitEnvironmentError> {
        // A certificate must cover every applicable impl, including poisoned candidates.
        let mut unique = HashMap::new();
        let mut ordered_traits = Vec::new();
        for implementation in implementations.values() {
            let target = implementation.header.trait_ref().declaration();
            match unique.entry(target.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    ordered_traits.push(target.clone());
                    entry.insert(Some(implementation));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }

        let mut chains = WrapperChains::new(signatures.types());
        let mut edges = Vec::new();
        let mut indices = HashMap::new();
        for target in ordered_traits {
            if let Some(implementation) = unique[&target]
                && let Some(edge) = wrapper_edge(implementation, signatures, &mut chains)?
            {
                indices.insert(target.clone(), edges.len());
                edges.push((target, edge));
            }
        }
        let successors = edges
            .iter()
            .map(|(_, edge)| {
                indices
                    .get(&edge.required)
                    .copied()
                    .filter(|&index| edges[index].1.arity == edge.arity)
            })
            .collect::<Vec<_>>();
        let growth = edges.iter().map(|(_, edge)| edge.grows).collect::<Vec<_>>();
        let classification = classify_cycles(&successors, &growth);
        let groups = edges
            .into_iter()
            .zip(classification.groups)
            .filter_map(|((target, _), group)| group.map(|group| (target, group)))
            .collect();
        Ok(Self {
            groups,
            #[cfg(test)]
            wrapper_work: (chains.kind_reads, chains.substitution_reads),
        })
    }

    fn blockers(&self, assumptions: &[TraitAssumption]) -> CycleBlockers {
        let subjects = assumptions
            .iter()
            .filter_map(|assumption| {
                self.groups
                    .get(assumption.obligation.trait_ref().declaration())
                    .map(|&group| (group, assumption.obligation.subject()))
            })
            .collect();
        CycleBlockers { subjects }
    }
}

impl TraitEnvironment {
    pub(super) fn is_unprovable_growing_cycle(
        &mut self,
        owner: Option<DefId>,
        signatures: &DeclarationSignatures,
        obligation: &TraitObligation,
        assumptions: &[TraitAssumption],
    ) -> Result<bool, TraitEnvironmentError> {
        if self.growing_cycles.is_none() {
            self.growing_cycles = Some(GrowingCycles::collect(&self.implementations, signatures)?);
        }
        let cycles = self.growing_cycles.as_ref().expect("cycles initialized");
        let Some(&group) = cycles.groups.get(obligation.trait_ref().declaration()) else {
            return Ok(false);
        };
        let blocked = if let Some(owner) = owner {
            self.owner_cycle_blockers
                .entry(owner)
                .or_insert_with(|| cycles.blockers(assumptions))
                .subjects
                .contains(&(group, obligation.subject()))
        } else if let Some(cache) = &mut self.local_query_cache {
            cache
                .cycle_blockers
                .get_or_insert_with(|| cycles.blockers(assumptions))
                .subjects
                .contains(&(group, obligation.subject()))
        } else {
            cycles
                .blockers(assumptions)
                .subjects
                .contains(&(group, obligation.subject()))
        };
        Ok(!blocked)
    }
}

fn wrapper_edge(
    implementation: &TraitImpl,
    signatures: &DeclarationSignatures,
    chains: &mut WrapperChains<'_>,
) -> Result<Option<WrapperEdge>, TraitEnvironmentError> {
    let header = &implementation.header;
    if implementation.poison.is_some() || !implementation.bindings.is_empty() {
        return Ok(None);
    }
    let CanonicalTarget::Source(_) = header.trait_ref().declaration() else {
        return Ok(None);
    };
    let FactState::Known(DeclarationSignature::Trait {
        generic_parameters,
        associated_types,
        ..
    }) = signatures.declaration(header.trait_ref().declaration())
    else {
        return Ok(None);
    };
    if !associated_types.is_empty()
        || generic_parameters.iter().any(|parameter| {
            !matches!(signatures.generic_parameter(parameter), FactState::Known(signature) if signature.bounds().is_empty())
        })
    {
        return Ok(None);
    }

    let types = signatures.types();
    let TypeKind::GenericParameter(subject) = types.kind(header.target())? else {
        return Ok(None);
    };
    let arguments = types.substitution(header.trait_ref().substitution())?;
    if arguments.len() != generic_parameters.len() {
        return Ok(None);
    }
    let mut slots = HashSet::from([subject.clone()]);
    let mut argument_slots = Vec::with_capacity(arguments.len());
    for &argument in arguments {
        let TypeKind::GenericParameter(parameter) = types.kind(argument)? else {
            return Ok(None);
        };
        if !slots.insert(parameter.clone()) {
            return Ok(None);
        }
        argument_slots.push(argument);
    }
    if slots.len() != header.generic_parameters().len() {
        return Ok(None);
    }

    let mut required = None;
    for parameter in header.generic_parameters() {
        if !slots.contains(parameter) {
            return Ok(None);
        }
        let FactState::Known(signature) = signatures.generic_parameter(parameter) else {
            return Ok(None);
        };
        if parameter == subject {
            let [bound] = signature.declared_bounds() else {
                return Ok(None);
            };
            required = Some(bound);
        } else if !signature.bounds().is_empty() {
            return Ok(None);
        }
    }
    let Some(required) = required else {
        return Ok(None);
    };
    let required_arguments = types.substitution(required.substitution())?;
    if required_arguments.len() != arguments.len() {
        return Ok(None);
    }
    let mut grows = false;
    for (&argument, slot) in required_arguments.iter().zip(argument_slots) {
        let Some(argument_grows) = chains.match_slot(argument, slot)? else {
            return Ok(None);
        };
        grows |= argument_grows;
    }
    Ok(Some(WrapperEdge {
        required: required.declaration().clone(),
        arity: arguments.len(),
        grows,
    }))
}

#[derive(Clone, Copy)]
struct WrapperTail {
    terminal: TypeId,
    grows: bool,
}

struct WrapperChains<'store> {
    types: &'store CanonicalTypeStore,
    tails: BTreeMap<TypeId, Option<WrapperTail>>,
    #[cfg(test)]
    kind_reads: usize,
    #[cfg(test)]
    substitution_reads: usize,
}

impl<'store> WrapperChains<'store> {
    fn new(types: &'store CanonicalTypeStore) -> Self {
        Self {
            types,
            tails: BTreeMap::new(),
            #[cfg(test)]
            kind_reads: 0,
            #[cfg(test)]
            substitution_reads: 0,
        }
    }

    fn match_slot(
        &mut self,
        root: TypeId,
        slot: TypeId,
    ) -> Result<Option<bool>, crate::TypeStoreError> {
        let mut path = Vec::new();
        let mut current = root;
        let mut tail = loop {
            if let Some(&tail) = self.tails.get(&current) {
                break tail;
            }
            #[cfg(test)]
            {
                self.kind_reads += 1;
            }
            let tail = match self.types.kind(current)? {
                TypeKind::Nominal {
                    head: NominalHead::Intrinsic(IntrinsicNominal::Opt),
                    substitution,
                } => {
                    #[cfg(test)]
                    {
                        self.substitution_reads += 1;
                    }
                    match self.types.substitution(*substitution)? {
                        [inner] => {
                            path.push(current);
                            current = *inner;
                            continue;
                        }
                        _ => None,
                    }
                }
                _ => Some(WrapperTail {
                    terminal: current,
                    grows: false,
                }),
            };
            self.tails.insert(current, tail);
            break tail;
        };

        // Summaries are slot-independent and published only after a complete walk.
        for parent in path.into_iter().rev() {
            if let Some(tail) = &mut tail {
                tail.grows = true;
            }
            self.tails.insert(parent, tail);
        }
        Ok(tail.and_then(|tail| (tail.terminal == slot).then_some(tail.grows)))
    }
}

#[derive(Clone, Copy)]
enum Visit {
    Unseen,
    Active(usize),
    Complete,
}

struct CycleClassification {
    groups: Vec<Option<CycleId>>,
    #[cfg(test)]
    visits: usize,
}

fn classify_cycles(successors: &[Option<usize>], growth: &[bool]) -> CycleClassification {
    let mut visits = vec![Visit::Unseen; successors.len()];
    let mut groups = vec![None; successors.len()];
    let mut next_group = 0;
    #[cfg(test)]
    let mut visit_count = 0;

    for start in 0..successors.len() {
        if !matches!(visits[start], Visit::Unseen) {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            match visits[index] {
                Visit::Unseen => {
                    visits[index] = Visit::Active(path.len());
                    path.push(index);
                    current = successors[index];
                    #[cfg(test)]
                    {
                        visit_count += 1;
                    }
                }
                Visit::Active(offset) => {
                    let cycle = &path[offset..];
                    if cycle.iter().any(|&member| growth[member]) {
                        let group = CycleId(next_group);
                        next_group += 1;
                        for &member in cycle {
                            groups[member] = Some(group);
                        }
                    }
                    break;
                }
                Visit::Complete => break,
            }
        }
        for index in path {
            visits[index] = Visit::Complete;
        }
    }
    CycleClassification {
        groups,
        #[cfg(test)]
        visits: visit_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_closed_productive_cycles_get_shared_certificates() {
        let result = classify_cycles(
            &[Some(1), Some(2), Some(1), Some(4), None, Some(5), Some(5)],
            &[false, false, true, true, true, false, true],
        );
        assert_eq!(
            result.groups,
            [
                None,
                Some(CycleId(0)),
                Some(CycleId(0)),
                None,
                None,
                None,
                None
            ]
        );
        assert_eq!(result.visits, 7);
    }

    #[test]
    fn long_cycles_and_shared_prefixes_visit_each_edge_once() {
        for size in [10, 100, 1_000, 10_000] {
            let mut successors = (1..size).map(Some).collect::<Vec<_>>();
            successors.push(Some(0));
            let mut growth = vec![false; size];
            growth[size - 1] = true;
            let result = classify_cycles(&successors, &growth);
            assert_eq!(result.visits, size);
            assert!(result.groups.iter().all(|group| *group == Some(CycleId(0))));

            let prefixes = vec![Some(size); size];
            successors = prefixes;
            successors.push(Some(size));
            growth = vec![true; size + 1];
            let result = classify_cycles(&successors, &growth);
            assert_eq!(result.visits, size + 1);
            assert!(result.groups[..size].iter().all(Option::is_none));
            assert_eq!(result.groups[size], Some(CycleId(0)));
        }
    }

    #[test]
    fn small_graphs_agree_with_independent_root_traversal() {
        for size in 1_usize..=4 {
            for encoding in 0..(size + 1).pow(size as u32) {
                let mut remaining = encoding;
                let successors = (0..size)
                    .map(|_| {
                        let successor = remaining % (size + 1);
                        remaining /= size + 1;
                        (successor < size).then_some(successor)
                    })
                    .collect::<Vec<_>>();
                for mask in 0..(1 << size) {
                    let growth = (0..size)
                        .map(|index| mask & (1 << index) != 0)
                        .collect::<Vec<_>>();
                    let result = classify_cycles(&successors, &growth);
                    assert_eq!(result.visits, size);
                    for start in 0..size {
                        let mut seen = HashSet::new();
                        let mut current = Some(start);
                        let mut grows = false;
                        while let Some(index) = current {
                            if !seen.insert(index) {
                                break;
                            }
                            grows |= growth[index];
                            current = successors[index];
                        }
                        let expected = current == Some(start) && grows;
                        assert_eq!(
                            result.groups[start].is_some(),
                            expected,
                            "{successors:?} {growth:?} {start}"
                        );
                        if let Some(group) = result.groups[start] {
                            assert!(
                                seen.into_iter()
                                    .all(|index| result.groups[index] == Some(group))
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod wrapper_tests;

use std::collections::{BTreeMap, BTreeSet};

use eglur_diagnostics::Poison;

use crate::{
    AssociatedBindingSignature, AssociatedProjection, CanonicalTarget, CanonicalTypeStore,
    DeclarationSignatures, DefId, FactState, GenericParameterId, TraitImplHeader, TraitRef, TypeId,
    TypeKind, WitnessSlotId,
};

use super::error::TraitEnvironmentError;
use super::model::{
    ImplArgumentViolation, TraitImplEvidence, TraitNormalization, TraitObligation, TraitProof,
    TraitReceiverCandidate,
};
use super::overlap::{
    ImplBuckets, ImplCandidate, ImplTargetBucket, classify_normalized, match_impl, match_type,
    target_bucket,
};

mod applications;
mod assumptions;
mod growing_cycles;
mod normalization;
#[cfg(test)]
mod tests;
pub(super) use applications::ApplicationCheck;
use assumptions::AssumptionNormalization;
use growing_cycles::{CycleBlockers, GrowingCycles};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TraitAssumption {
    pub(super) obligation: TraitObligation,
    pub(super) evidence: TraitAssumptionEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TraitAssumptionEvidence {
    Witness(WitnessSlotId),
    Trusted,
    Implementation(CanonicalTarget),
}

#[derive(Clone, Debug)]
pub(super) struct TraitImpl {
    pub(super) header: TraitImplHeader,
    pub(super) bindings: BTreeMap<CanonicalTarget, AssociatedBindingSignature>,
    pub(super) poison: Option<Poison>,
}

#[derive(Clone, Debug)]
struct ImplSelection {
    implementation: CanonicalTarget,
    replacements: BTreeMap<GenericParameterId, TypeId>,
}

#[derive(Clone, Debug)]
enum SelectionOutcome {
    Assumed(WitnessSlotId),
    TrustedBound,
    Selected(ImplSelection),
    Poisoned(Poison),
    InvalidArgument(ImplArgumentViolation),
    Unsatisfied(TraitObligation),
    Ambiguous(TraitObligation),
}

impl SelectionOutcome {
    fn normalization_failure(failure: TraitNormalization) -> Self {
        match failure {
            TraitNormalization::Poisoned(poison) => Self::Poisoned(poison),
            TraitNormalization::InvalidArgument(violation) => Self::InvalidArgument(violation),
            TraitNormalization::Unsatisfied(obligation)
            | TraitNormalization::Ambiguous(obligation) => Self::Unsatisfied(obligation),
            TraitNormalization::Cyclic(projection) => Self::Unsatisfied(TraitObligation::new(
                projection.base(),
                projection.trait_ref().clone(),
            )),
            TraitNormalization::Normalized(_) | TraitNormalization::Symbolic(_) => {
                unreachable!("normalization success has an obligation")
            }
        }
    }
}

#[derive(Default)]
struct LocalQueryCache {
    projections: BTreeMap<TypeId, TraitNormalization>,
    selections: BTreeMap<TraitObligation, SelectionOutcome>,
    cycle_blockers: Option<CycleBlockers>,
}

pub(super) enum CollectedImplementations {
    Complete(BTreeMap<CanonicalTarget, TraitImpl>),
    Unperformed,
}

pub(super) struct ImplementationValidationInput {
    pub(super) implementation: CanonicalTarget,
    pub(super) header: TraitImplHeader,
    pub(super) existing_poison: Option<Poison>,
}

/// Checked impl index and bounded associated-projection normalizer.
pub struct TraitEnvironment {
    local_query_cache: Option<LocalQueryCache>,
    implementations: BTreeMap<CanonicalTarget, TraitImpl>,
    buckets: ImplBuckets,
    assumptions: BTreeMap<DefId, Box<[TraitAssumption]>>,
    owner_assumption_cache: BTreeMap<DefId, Vec<TraitAssumption>>,
    growing_cycles: Option<GrowingCycles>,
    owner_cycle_blockers: BTreeMap<DefId, CycleBlockers>,
    method_targets: BTreeMap<(CanonicalTarget, CanonicalTarget), CanonicalTarget>,
    normalization_cache: BTreeMap<(DefId, TypeId), TraitNormalization>,
    projection_cache: BTreeMap<(DefId, TypeId), TraitNormalization>,
    selection_cache: BTreeMap<(DefId, TraitObligation), SelectionOutcome>,
}

impl TraitEnvironment {
    pub(super) fn from_checked_parts(
        implementations: BTreeMap<CanonicalTarget, TraitImpl>,
        buckets: ImplBuckets,
        assumptions: BTreeMap<DefId, Box<[TraitAssumption]>>,
    ) -> Self {
        Self {
            local_query_cache: None,
            implementations,
            buckets,
            assumptions,
            owner_assumption_cache: BTreeMap::new(),
            growing_cycles: None,
            owner_cycle_blockers: BTreeMap::new(),
            method_targets: BTreeMap::new(),
            normalization_cache: BTreeMap::new(),
            projection_cache: BTreeMap::new(),
            selection_cache: BTreeMap::new(),
        }
    }

    pub(crate) fn witness_obligation(&self, witness: WitnessSlotId) -> Option<&TraitObligation> {
        self.assumptions
            .get(&witness.owner())?
            .iter()
            .find_map(|assumption| match assumption.evidence {
                TraitAssumptionEvidence::Witness(candidate) if candidate == witness => {
                    Some(&assumption.obligation)
                }
                TraitAssumptionEvidence::Witness(_)
                | TraitAssumptionEvidence::Trusted
                | TraitAssumptionEvidence::Implementation(_) => None,
            })
    }

    pub(crate) fn declared_witness_for_obligation(
        &self,
        owner: DefId,
        obligation: &TraitObligation,
    ) -> Option<WitnessSlotId> {
        self.assumptions.get(&owner)?.iter().find_map(|assumption| {
            if assumption.obligation != *obligation {
                return None;
            }
            match assumption.evidence {
                TraitAssumptionEvidence::Witness(witness) => Some(witness),
                TraitAssumptionEvidence::Implementation(_) | TraitAssumptionEvidence::Trusted => {
                    None
                }
            }
        })
    }

    pub(super) fn poison_implementation(
        &mut self,
        implementation: &CanonicalTarget,
        poison: Poison,
    ) {
        self.implementations
            .get_mut(implementation)
            .expect("collected implementation remains present")
            .poison = Some(poison);
        self.method_targets
            .retain(|(owner, _), _| owner != implementation);
        self.clear_query_caches();
    }

    pub(super) fn implementation_validation_inputs(&self) -> Vec<ImplementationValidationInput> {
        self.implementations
            .iter()
            .map(|(target, implementation)| ImplementationValidationInput {
                implementation: target.clone(),
                header: implementation.header.clone(),
                existing_poison: implementation.poison.clone(),
            })
            .collect()
    }

    pub(super) fn extend_method_targets(
        &mut self,
        targets: BTreeMap<(CanonicalTarget, CanonicalTarget), CanonicalTarget>,
    ) {
        self.method_targets.extend(targets);
    }

    pub(super) fn clear_query_caches(&mut self) {
        self.owner_assumption_cache.clear();
        self.growing_cycles = None;
        self.owner_cycle_blockers.clear();
        self.normalization_cache.clear();
        self.projection_cache.clear();
        self.selection_cache.clear();
        self.local_query_cache = None;
    }

    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.implementations.len()
            + self
                .buckets
                .values()
                .map(|bucket| bucket.len())
                .sum::<usize>()
            + self
                .assumptions
                .values()
                .map(|values| values.len())
                .sum::<usize>()
            + self.method_targets.len()
    }

    pub(crate) fn has_poisoned_implementations(&self) -> bool {
        self.implementations
            .values()
            .any(|implementation| implementation.poison.is_some())
    }

    pub(crate) fn checked_implementation_contract(
        &self,
        implementation: &CanonicalTarget,
    ) -> Option<(
        &TraitImplHeader,
        &BTreeMap<CanonicalTarget, AssociatedBindingSignature>,
    )> {
        let implementation = self.implementations.get(implementation)?;
        implementation
            .poison
            .is_none()
            .then_some((&implementation.header, &implementation.bindings))
    }

    pub(crate) fn method_targets(
        &self,
    ) -> impl Iterator<Item = (&CanonicalTarget, &CanonicalTarget, &CanonicalTarget)> {
        self.method_targets
            .iter()
            .map(|((implementation, trait_method), target)| (implementation, trait_method, target))
    }

    #[must_use]
    pub fn normalization_cache_len(&self) -> usize {
        self.normalization_cache.len() + self.projection_cache.len()
    }

    /// Returns the implementation method selected by the checked trait environment.
    #[must_use]
    pub fn checked_impl_method(
        &self,
        implementation: &CanonicalTarget,
        trait_method: &CanonicalTarget,
    ) -> Option<&CanonicalTarget> {
        self.checked_impl_target(implementation)?;
        self.method_targets
            .get(&(implementation.clone(), trait_method.clone()))
    }

    /// Returns the normalized target type of one valid checked implementation.
    #[must_use]
    pub fn checked_impl_target(&self, implementation: &CanonicalTarget) -> Option<TypeId> {
        self.implementations
            .get(implementation)
            .filter(|implementation| implementation.poison.is_none())
            .map(|implementation| implementation.header.target())
    }

    pub fn normalize_for_owner(
        &mut self,
        owner: DefId,
        signatures: &mut DeclarationSignatures,
        root: TypeId,
    ) -> Result<TraitNormalization, TraitEnvironmentError> {
        if let FactState::Poisoned(poison) = signatures.source_declaration(owner) {
            return Ok(TraitNormalization::Poisoned(poison.clone()));
        }
        if let Some(cached) = self.normalization_cache.get(&(owner, root)) {
            return Ok(cached.clone());
        }
        let assumptions = match self.normalized_owner_assumptions(owner, signatures)? {
            Ok(assumptions) => assumptions,
            Err(failure) => return Ok(failure),
        };
        let mut active_projections = BTreeSet::new();
        let mut active_obligations = BTreeSet::new();
        let result = self.normalize_inner(
            Some(owner),
            signatures,
            root,
            &assumptions,
            &mut active_projections,
            &mut active_obligations,
        )?;
        self.normalization_cache
            .insert((owner, root), result.clone());
        Ok(result)
    }

    pub(super) fn normalize_for_implementation(
        &mut self,
        implementation: &CanonicalTarget,
        signatures: &mut DeclarationSignatures,
        root: TypeId,
    ) -> Result<TraitNormalization, TraitEnvironmentError> {
        let implementation_fact = self
            .implementations
            .get(implementation)
            .cloned()
            .ok_or_else(|| TraitEnvironmentError::UnknownImplementation(implementation.clone()))?;
        if let Some(poison) = implementation_fact.poison {
            return Ok(TraitNormalization::Poisoned(poison));
        }
        let assumptions = [TraitAssumption {
            obligation: TraitObligation {
                subject: implementation_fact.header.target(),
                trait_ref: implementation_fact.header.trait_ref().clone(),
            },
            evidence: TraitAssumptionEvidence::Implementation(implementation.clone()),
        }];
        self.local_query_cache = Some(LocalQueryCache::default());
        let result = self.normalize_inner(
            None,
            signatures,
            root,
            &assumptions,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        );
        self.local_query_cache = None;
        result
    }

    /// Proves one exact obligation using the checked environment and owner assumptions.
    pub fn prove_for_owner(
        &mut self,
        owner: DefId,
        signatures: &mut DeclarationSignatures,
        obligation: TraitObligation,
    ) -> Result<TraitProof, TraitEnvironmentError> {
        let outcome = match self.normalized_owner_assumptions(owner, signatures)? {
            Err(failure) => SelectionOutcome::normalization_failure(failure),
            Ok(assumptions) => {
                let mut active = BTreeSet::new();
                match self.normalize_obligation(
                    Some(owner),
                    signatures,
                    &obligation,
                    &assumptions,
                    &mut active,
                )? {
                    Ok(obligation) => self.select_impl(
                        Some(owner),
                        signatures,
                        obligation,
                        &assumptions,
                        &mut active,
                    )?,
                    Err(failure) => SelectionOutcome::normalization_failure(failure),
                }
            }
        };
        Ok(match outcome {
            SelectionOutcome::Assumed(witness) => TraitProof::Bound(witness),
            SelectionOutcome::TrustedBound => {
                unreachable!("source owners only have source witness slots")
            }
            SelectionOutcome::Selected(selection) => {
                let implementation = self
                    .implementations
                    .get(&selection.implementation)
                    .ok_or_else(|| {
                        TraitEnvironmentError::UnknownImplementation(
                            selection.implementation.clone(),
                        )
                    })?;
                let arguments = implementation
                    .header
                    .generic_parameters()
                    .iter()
                    .map(|parameter| {
                        selection
                            .replacements
                            .get(parameter)
                            .copied()
                            .ok_or_else(|| TraitEnvironmentError::UnconstrainedCheckedImpl {
                                implementation: selection.implementation.clone(),
                                parameter: parameter.clone(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let substitution = signatures.types_mut().intern_substitution(arguments)?;
                TraitProof::Implementation(TraitImplEvidence {
                    implementation: selection.implementation,
                    substitution,
                })
            }
            SelectionOutcome::Poisoned(poison) => TraitProof::Poisoned(poison),
            SelectionOutcome::InvalidArgument(violation) => TraitProof::InvalidArgument(violation),
            SelectionOutcome::Unsatisfied(obligation) => TraitProof::Unsatisfied(obligation),
            SelectionOutcome::Ambiguous(obligation) => TraitProof::Ambiguous(obligation),
        })
    }

    /// Collects structural receiver matches without using impl bounds as filters.
    pub(crate) fn receiver_candidates_for_trait(
        &mut self,
        owner: DefId,
        signatures: &mut DeclarationSignatures,
        subject: TypeId,
        trait_declaration: &CanonicalTarget,
    ) -> Result<Result<Vec<TraitReceiverCandidate>, TraitNormalization>, TraitEnvironmentError>
    {
        let assumptions = match self.normalized_owner_assumptions(owner, signatures)? {
            Ok(assumptions) => assumptions,
            Err(failure) => return Ok(Err(failure)),
        };
        let mut assumed = assumptions
            .iter()
            .filter(|assumption| {
                assumption.obligation.subject == subject
                    && assumption.obligation.trait_ref.declaration() == trait_declaration
            })
            .map(|assumption| TraitReceiverCandidate {
                trait_ref: assumption.obligation.trait_ref.clone(),
                generic_parameters: Box::new([]),
                replacements: BTreeMap::new(),
            })
            .collect::<Vec<_>>();
        if !assumed.is_empty() {
            assumed.sort();
            assumed.dedup();
            return Ok(Ok(assumed));
        }

        let target_bucket = target_bucket(signatures.types(), subject)?;
        let mut declarations = Vec::new();
        for bucket in [target_bucket, ImplTargetBucket::Blanket] {
            if let Some(found) = self.buckets.get(&(trait_declaration.clone(), bucket)) {
                declarations.extend(found.iter().cloned());
            }
        }
        declarations.sort();
        declarations.dedup();
        let mut candidates = Vec::new();
        for declaration in declarations {
            let implementation = self
                .implementations
                .get(&declaration)
                .ok_or_else(|| TraitEnvironmentError::UnknownImplementation(declaration.clone()))?;
            let parameters = implementation
                .header
                .generic_parameters()
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let mut replacements = BTreeMap::new();
            if !match_type(
                signatures.types(),
                implementation.header.target(),
                subject,
                &parameters,
                &mut replacements,
            )? {
                continue;
            }
            let arguments = signatures
                .types()
                .substitution(implementation.header.trait_ref().substitution())?
                .to_vec()
                .into_iter()
                .map(|argument| signatures.types_mut().substitute(argument, &replacements))
                .collect::<Result<Vec<_>, _>>()?;
            let substitution = signatures.types_mut().intern_substitution(arguments)?;
            candidates.push(TraitReceiverCandidate {
                trait_ref: TraitRef::new(trait_declaration.clone(), substitution),
                generic_parameters: implementation
                    .header
                    .generic_parameters()
                    .to_vec()
                    .into_boxed_slice(),
                replacements,
            });
        }
        candidates.sort();
        candidates.dedup();
        Ok(Ok(candidates))
    }

    fn select_impl(
        &mut self,
        owner: Option<DefId>,
        signatures: &mut DeclarationSignatures,
        obligation: TraitObligation,
        assumptions: &[TraitAssumption],
        active: &mut BTreeSet<TraitObligation>,
    ) -> Result<SelectionOutcome, TraitEnvironmentError> {
        if let FactState::Poisoned(poison) =
            signatures.declaration(obligation.trait_ref.declaration())
        {
            return Ok(SelectionOutcome::Poisoned(poison.clone()));
        }
        if let Some(owner) = owner
            && let Some(cached) = self.selection_cache.get(&(owner, obligation.clone()))
        {
            return Ok(cached.clone());
        }
        if owner.is_none()
            && let Some(cached) = self
                .local_query_cache
                .as_ref()
                .and_then(|cache| cache.selections.get(&obligation))
        {
            return Ok(cached.clone());
        }
        if let Some(assumption) = assumptions
            .iter()
            .find(|assumption| assumption.obligation == obligation)
        {
            let selected = match &assumption.evidence {
                TraitAssumptionEvidence::Implementation(evidence) => {
                    let implementation = self.implementations.get(evidence).ok_or_else(|| {
                        TraitEnvironmentError::UnknownImplementation(evidence.clone())
                    })?;
                    if let Some(poison) = &implementation.poison {
                        return Ok(SelectionOutcome::Poisoned(poison.clone()));
                    }
                    match match_impl(signatures.types(), &implementation.header, &obligation)? {
                        Some(replacements) => match impl_argument_violation(
                            signatures.types(),
                            &implementation.header,
                            &replacements,
                        )? {
                            Some(violation) => SelectionOutcome::InvalidArgument(violation),
                            None => SelectionOutcome::Selected(ImplSelection {
                                implementation: evidence.clone(),
                                replacements,
                            }),
                        },
                        None => SelectionOutcome::Unsatisfied(obligation.clone()),
                    }
                }
                TraitAssumptionEvidence::Witness(witness) => SelectionOutcome::Assumed(*witness),
                TraitAssumptionEvidence::Trusted => SelectionOutcome::TrustedBound,
            };
            if let Some(owner) = owner {
                self.selection_cache
                    .insert((owner, obligation), selected.clone());
            } else if let Some(cache) = &mut self.local_query_cache {
                cache.selections.insert(obligation, selected.clone());
            }
            return Ok(selected);
        }
        if !active.insert(obligation.clone()) {
            return Ok(SelectionOutcome::Unsatisfied(obligation));
        }

        let mut candidates = self.candidates(signatures.types(), &obligation)?;
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        let mut rejected = None;
        let mut viable = Vec::new();
        for candidate in candidates {
            if let Some(poison) = self
                .implementations
                .get(&candidate.0)
                .and_then(|implementation| implementation.poison.as_ref())
            {
                rejected = Some(match rejected {
                    Some(existing) => Poison::merge(&existing, poison)?,
                    None => poison.clone(),
                });
            } else {
                viable.push(candidate);
            }
        }
        let outcome = if viable.is_empty() {
            rejected.clone().map_or_else(
                || SelectionOutcome::Unsatisfied(obligation.clone()),
                SelectionOutcome::Poisoned,
            )
        } else if viable.len() > 1 {
            SelectionOutcome::Ambiguous(obligation.clone())
        } else {
            let (declaration, replacements) = viable.pop().expect("one viable candidate exists");
            let implementation = self
                .implementations
                .get(&declaration)
                .cloned()
                .ok_or_else(|| TraitEnvironmentError::UnknownImplementation(declaration.clone()))?;
            if let Some(poison) = implementation.poison {
                SelectionOutcome::Poisoned(poison)
            } else if let Some(violation) =
                impl_argument_violation(signatures.types(), &implementation.header, &replacements)?
            {
                SelectionOutcome::InvalidArgument(violation)
            } else if self.is_unprovable_growing_cycle(
                owner,
                signatures,
                &obligation,
                assumptions,
            )? {
                SelectionOutcome::Unsatisfied(obligation.clone())
            } else {
                let mut failed = None;
                for parameter in implementation.header.generic_parameters() {
                    let signature = match signatures.generic_parameter(parameter) {
                        FactState::Known(signature) => signature.clone(),
                        FactState::Poisoned(poison) => {
                            failed = Some(SelectionOutcome::Poisoned(poison.clone()));
                            break;
                        }
                        FactState::Unperformed | FactState::NotApplicable => {
                            return Err(TraitEnvironmentError::UnavailableGenericParameter(
                                parameter.clone(),
                            ));
                        }
                    };
                    if !replacements.contains_key(parameter) {
                        return Err(TraitEnvironmentError::UnconstrainedCheckedImpl {
                            implementation: declaration,
                            parameter: parameter.clone(),
                        });
                    }
                    for bound in
                        signature.instantiate_bounds(signatures.types_mut(), &replacements)?
                    {
                        let required = bound.into_obligation();
                        let required = match self.normalize_obligation(
                            owner,
                            signatures,
                            &required,
                            assumptions,
                            active,
                        )? {
                            Ok(required) => required,
                            Err(failure) => {
                                failed = Some(SelectionOutcome::normalization_failure(failure));
                                break;
                            }
                        };
                        match self.select_impl(
                            owner,
                            signatures,
                            required.clone(),
                            assumptions,
                            active,
                        )? {
                            SelectionOutcome::Assumed(_)
                            | SelectionOutcome::TrustedBound
                            | SelectionOutcome::Selected(_) => {}
                            SelectionOutcome::Poisoned(poison) => {
                                failed = Some(SelectionOutcome::Poisoned(poison));
                                break;
                            }
                            SelectionOutcome::InvalidArgument(violation) => {
                                failed = Some(SelectionOutcome::InvalidArgument(violation));
                                break;
                            }
                            SelectionOutcome::Unsatisfied(obligation)
                            | SelectionOutcome::Ambiguous(obligation) => {
                                failed = Some(SelectionOutcome::Unsatisfied(obligation));
                                break;
                            }
                        }
                    }
                    if failed.is_some() {
                        break;
                    }
                }
                failed.unwrap_or(SelectionOutcome::Selected(ImplSelection {
                    implementation: declaration,
                    replacements,
                }))
            }
        };
        let outcome = match (outcome, rejected) {
            (SelectionOutcome::Unsatisfied(_) | SelectionOutcome::Ambiguous(_), Some(poison)) => {
                SelectionOutcome::Poisoned(poison)
            }
            (outcome, _) => outcome,
        };
        active.remove(&obligation);
        if let Some(owner) = owner {
            self.selection_cache
                .insert((owner, obligation), outcome.clone());
        } else if let Some(cache) = &mut self.local_query_cache {
            cache.selections.insert(obligation, outcome.clone());
        }
        Ok(outcome)
    }

    fn candidates(
        &self,
        types: &CanonicalTypeStore,
        obligation: &TraitObligation,
    ) -> Result<Vec<ImplCandidate>, TraitEnvironmentError> {
        let target_bucket = target_bucket(types, obligation.subject)?;
        let trait_declaration = obligation.trait_ref.declaration().clone();
        let mut declarations = Vec::new();
        for bucket in [target_bucket, ImplTargetBucket::Blanket] {
            if let Some(found) = self.buckets.get(&(trait_declaration.clone(), bucket)) {
                declarations.extend(found.iter().cloned());
            }
        }
        declarations.sort();
        declarations.dedup();
        let mut candidates = Vec::new();
        for declaration in declarations {
            let implementation = self
                .implementations
                .get(&declaration)
                .ok_or_else(|| TraitEnvironmentError::UnknownImplementation(declaration.clone()))?;
            if let Some(replacements) = match_impl(types, &implementation.header, obligation)? {
                candidates.push((declaration, replacements));
            }
        }
        Ok(candidates)
    }
}

fn impl_argument_violation(
    types: &CanonicalTypeStore,
    header: &TraitImplHeader,
    replacements: &BTreeMap<GenericParameterId, TypeId>,
) -> Result<Option<ImplArgumentViolation>, TraitEnvironmentError> {
    let arguments = header
        .generic_parameters()
        .iter()
        .map(|parameter| {
            replacements.get(parameter).copied().ok_or_else(|| {
                TraitEnvironmentError::UnconstrainedCheckedImpl {
                    implementation: header.declaration().clone(),
                    parameter: parameter.clone(),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ImplArgumentViolation::in_arguments(types, &arguments)?)
}

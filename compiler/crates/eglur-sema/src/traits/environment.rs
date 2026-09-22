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
pub(super) use applications::ApplicationCheck;

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

#[derive(Default)]
struct LocalQueryCache {
    projections: BTreeMap<TypeId, TraitNormalization>,
    selections: BTreeMap<TraitObligation, SelectionOutcome>,
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

    pub(crate) fn witness_for_obligation(
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
        let assumptions = self
            .assumptions
            .get(&owner)
            .map_or_else(Vec::new, |values| values.to_vec());
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
        if let FactState::Poisoned(poison) = signatures.source_declaration(owner) {
            return Ok(TraitProof::Poisoned(poison.clone()));
        }
        let assumptions = self
            .assumptions
            .get(&owner)
            .map_or_else(Vec::new, |values| values.to_vec());
        let mut active = BTreeSet::new();
        Ok(
            match self.select_impl(
                Some(owner),
                signatures,
                obligation,
                &assumptions,
                &mut active,
            )? {
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
                SelectionOutcome::InvalidArgument(violation) => {
                    TraitProof::InvalidArgument(violation)
                }
                SelectionOutcome::Unsatisfied(obligation) => TraitProof::Unsatisfied(obligation),
                SelectionOutcome::Ambiguous(obligation) => TraitProof::Ambiguous(obligation),
            },
        )
    }

    /// Collects structural receiver matches without using impl bounds as filters.
    pub(crate) fn receiver_candidates_for_trait(
        &self,
        owner: DefId,
        signatures: &mut DeclarationSignatures,
        subject: TypeId,
        trait_declaration: &CanonicalTarget,
    ) -> Result<Vec<TraitReceiverCandidate>, TraitEnvironmentError> {
        let assumptions = self.assumptions.get(&owner).map_or(&[][..], AsRef::as_ref);
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
            return Ok(assumed);
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
        Ok(candidates)
    }

    fn normalize_inner(
        &mut self,
        owner: Option<DefId>,
        signatures: &mut DeclarationSignatures,
        root: TypeId,
        assumptions: &[TraitAssumption],
        active_projections: &mut BTreeSet<TypeId>,
        active_obligations: &mut BTreeSet<TraitObligation>,
    ) -> Result<TraitNormalization, TraitEnvironmentError> {
        let kind = signatures.types().kind(root)?.clone();
        match kind {
            TypeKind::Never
            | TypeKind::Primitive(_)
            | TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_) => Ok(TraitNormalization::Normalized(root)),
            TypeKind::Nominal { head, substitution } => {
                let arguments = signatures.types().substitution(substitution)?.to_vec();
                let (arguments, symbolic) = match self.normalize_many(
                    owner,
                    signatures,
                    &arguments,
                    assumptions,
                    active_projections,
                    active_obligations,
                )? {
                    Ok(normalized) => normalized,
                    Err(failure) => return Ok(failure),
                };
                let ty = signatures.types_mut().nominal(head, arguments)?;
                Ok(classify_normalized(ty, symbolic))
            }
            TypeKind::Reference {
                mutability,
                referent,
            } => {
                let (referent, symbolic) = match self.normalize_inner(
                    owner,
                    signatures,
                    referent,
                    assumptions,
                    active_projections,
                    active_obligations,
                )? {
                    TraitNormalization::Normalized(ty) => (ty, false),
                    TraitNormalization::Symbolic(ty) => (ty, true),
                    failure => return Ok(failure),
                };
                let ty = signatures.types_mut().reference(mutability, referent)?;
                Ok(classify_normalized(ty, symbolic))
            }
            TypeKind::RawPointer {
                mutability,
                referent,
            } => {
                let (referent, symbolic) = match self.normalize_inner(
                    owner,
                    signatures,
                    referent,
                    assumptions,
                    active_projections,
                    active_obligations,
                )? {
                    TraitNormalization::Normalized(ty) => (ty, false),
                    TraitNormalization::Symbolic(ty) => (ty, true),
                    failure => return Ok(failure),
                };
                let ty = signatures.types_mut().raw_pointer(mutability, referent)?;
                Ok(classify_normalized(ty, symbolic))
            }
            TypeKind::Tuple(elements) => {
                let (elements, symbolic) = match self.normalize_many(
                    owner,
                    signatures,
                    &elements,
                    assumptions,
                    active_projections,
                    active_obligations,
                )? {
                    Ok(normalized) => normalized,
                    Err(failure) => return Ok(failure),
                };
                let ty = signatures.types_mut().tuple(elements)?;
                Ok(classify_normalized(ty, symbolic))
            }
            TypeKind::Callable { parameters, result } => {
                let (parameters, mut symbolic) = match self.normalize_many(
                    owner,
                    signatures,
                    &parameters,
                    assumptions,
                    active_projections,
                    active_obligations,
                )? {
                    Ok(normalized) => normalized,
                    Err(failure) => return Ok(failure),
                };
                let result = match self.normalize_inner(
                    owner,
                    signatures,
                    result,
                    assumptions,
                    active_projections,
                    active_obligations,
                )? {
                    TraitNormalization::Normalized(ty) => ty,
                    TraitNormalization::Symbolic(ty) => {
                        symbolic = true;
                        ty
                    }
                    failure => return Ok(failure),
                };
                let ty = signatures.types_mut().callable(parameters, result)?;
                Ok(classify_normalized(ty, symbolic))
            }
            TypeKind::ErrorUnion(members) => {
                let (members, symbolic) = match self.normalize_many(
                    owner,
                    signatures,
                    &members,
                    assumptions,
                    active_projections,
                    active_obligations,
                )? {
                    Ok(normalized) => normalized,
                    Err(failure) => return Ok(failure),
                };
                let ty = signatures.types_mut().error_union(members)?;
                Ok(classify_normalized(ty, symbolic))
            }
            TypeKind::AssociatedProjection(projection) => self.normalize_projection(
                owner,
                signatures,
                projection,
                assumptions,
                active_projections,
                active_obligations,
            ),
        }
    }

    fn normalize_many(
        &mut self,
        owner: Option<DefId>,
        signatures: &mut DeclarationSignatures,
        values: &[TypeId],
        assumptions: &[TraitAssumption],
        active_projections: &mut BTreeSet<TypeId>,
        active_obligations: &mut BTreeSet<TraitObligation>,
    ) -> Result<Result<(Vec<TypeId>, bool), TraitNormalization>, TraitEnvironmentError> {
        let mut normalized = Vec::with_capacity(values.len());
        let mut symbolic = false;
        for &value in values {
            match self.normalize_inner(
                owner,
                signatures,
                value,
                assumptions,
                active_projections,
                active_obligations,
            )? {
                TraitNormalization::Normalized(ty) => normalized.push(ty),
                TraitNormalization::Symbolic(ty) => {
                    symbolic = true;
                    normalized.push(ty);
                }
                failure @ (TraitNormalization::InvalidArgument(_)
                | TraitNormalization::Poisoned(_)
                | TraitNormalization::Unsatisfied(_)
                | TraitNormalization::Ambiguous(_)
                | TraitNormalization::Cyclic(_)) => return Ok(Err(failure)),
            }
        }
        Ok(Ok((normalized, symbolic)))
    }

    fn normalize_projection(
        &mut self,
        owner: Option<DefId>,
        signatures: &mut DeclarationSignatures,
        projection: AssociatedProjection,
        assumptions: &[TraitAssumption],
        active_projections: &mut BTreeSet<TypeId>,
        active_obligations: &mut BTreeSet<TraitObligation>,
    ) -> Result<TraitNormalization, TraitEnvironmentError> {
        let (base, _) = match self.normalize_inner(
            owner,
            signatures,
            projection.base(),
            assumptions,
            active_projections,
            active_obligations,
        )? {
            TraitNormalization::Normalized(ty) => (ty, false),
            TraitNormalization::Symbolic(ty) => (ty, true),
            failure => return Ok(failure),
        };
        let arguments = signatures
            .types()
            .substitution(projection.trait_ref().substitution())?
            .to_vec();
        let (arguments, _) = match self.normalize_many(
            owner,
            signatures,
            &arguments,
            assumptions,
            active_projections,
            active_obligations,
        )? {
            Ok(normalized) => normalized,
            Err(failure) => return Ok(failure),
        };
        let substitution = signatures.types_mut().intern_substitution(arguments)?;
        let trait_ref = TraitRef::new(projection.trait_ref().declaration().clone(), substitution);
        let normalized_projection =
            AssociatedProjection::new(base, trait_ref.clone(), projection.associated().clone());
        let projection_ty = signatures
            .types_mut()
            .associated_projection(normalized_projection.clone())?;

        let obligation = TraitObligation {
            subject: base,
            trait_ref,
        };
        let has_current_impl_evidence = assumptions.iter().any(|assumption| {
            assumption.obligation == obligation
                && matches!(
                    &assumption.evidence,
                    TraitAssumptionEvidence::Implementation(_)
                )
        });
        if matches!(
            signatures.types().kind(base)?,
            TypeKind::GenericParameter(_) | TypeKind::SelfType(_)
        ) && !has_current_impl_evidence
        {
            return Ok(TraitNormalization::Symbolic(projection_ty));
        }
        if let Some(owner) = owner
            && let Some(cached) = self.projection_cache.get(&(owner, projection_ty))
        {
            return Ok(cached.clone());
        }
        if owner.is_none()
            && let Some(cached) = self
                .local_query_cache
                .as_ref()
                .and_then(|cache| cache.projections.get(&projection_ty))
        {
            return Ok(cached.clone());
        }
        if !active_projections.insert(projection_ty) {
            return Ok(TraitNormalization::Cyclic(normalized_projection));
        }

        let selected = self.select_impl(
            owner,
            signatures,
            obligation.clone(),
            assumptions,
            active_obligations,
        )?;
        let result = match selected {
            SelectionOutcome::Selected(selection) => {
                let implementation = self
                    .implementations
                    .get(&selection.implementation)
                    .cloned()
                    .ok_or_else(|| {
                        TraitEnvironmentError::UnknownImplementation(
                            selection.implementation.clone(),
                        )
                    })?;
                let Some(binding) = implementation.bindings.get(projection.associated()) else {
                    active_projections.remove(&projection_ty);
                    return Err(TraitEnvironmentError::MissingAssociatedBinding {
                        implementation: selection.implementation,
                        associated: projection.associated().clone(),
                    });
                };
                let value = signatures
                    .types_mut()
                    .substitute(binding.value(), &selection.replacements)?;
                self.normalize_inner(
                    owner,
                    signatures,
                    value,
                    assumptions,
                    active_projections,
                    active_obligations,
                )?
            }
            SelectionOutcome::Poisoned(poison) => TraitNormalization::Poisoned(poison),
            SelectionOutcome::InvalidArgument(violation) => {
                TraitNormalization::InvalidArgument(violation)
            }
            SelectionOutcome::Unsatisfied(obligation) => {
                TraitNormalization::Unsatisfied(obligation)
            }
            SelectionOutcome::Ambiguous(obligation) => TraitNormalization::Ambiguous(obligation),
            SelectionOutcome::Assumed(_) | SelectionOutcome::TrustedBound => {
                TraitNormalization::Unsatisfied(obligation)
            }
        };
        active_projections.remove(&projection_ty);
        if let Some(owner) = owner {
            self.projection_cache
                .insert((owner, projection_ty), result.clone());
        } else if let Some(cache) = &mut self.local_query_cache {
            cache.projections.insert(projection_ty, result.clone());
        }
        Ok(result)
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
        let outcome = if candidates.is_empty() {
            SelectionOutcome::Unsatisfied(obligation.clone())
        } else if candidates.len() > 1 {
            let mut poison: Option<Poison> = None;
            for (implementation, _) in &candidates {
                if let Some(found) = self
                    .implementations
                    .get(implementation)
                    .and_then(|candidate| candidate.poison.as_ref())
                {
                    poison = Some(match poison {
                        Some(existing) => existing.merge(found)?,
                        None => found.clone(),
                    });
                }
            }
            poison.map_or_else(
                || SelectionOutcome::Ambiguous(obligation.clone()),
                SelectionOutcome::Poisoned,
            )
        } else {
            let (declaration, replacements) = candidates.pop().expect("one candidate exists");
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
            } else {
                let mut failed = None;
                for parameter in implementation.header.generic_parameters() {
                    let bounds = match signatures.declared_parameter_bounds(parameter) {
                        FactState::Known(bounds) => bounds.to_vec(),
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
                    let Some(&subject) = replacements.get(parameter) else {
                        return Err(TraitEnvironmentError::UnconstrainedCheckedImpl {
                            implementation: declaration,
                            parameter: parameter.clone(),
                        });
                    };
                    for bound in &bounds {
                        let arguments = signatures
                            .types()
                            .substitution(bound.substitution())?
                            .to_vec()
                            .into_iter()
                            .map(|argument| {
                                signatures.types_mut().substitute(argument, &replacements)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let substitution = signatures.types_mut().intern_substitution(arguments)?;
                        let required = TraitObligation {
                            subject,
                            trait_ref: TraitRef::new(bound.declaration().clone(), substitution),
                        };
                        let required = match self.normalize_obligation(
                            owner,
                            signatures,
                            &required,
                            assumptions,
                            active,
                        )? {
                            Ok(required) => required,
                            Err(failure) => {
                                failed = Some(match failure {
                                    TraitNormalization::Poisoned(poison) => {
                                        SelectionOutcome::Poisoned(poison)
                                    }
                                    TraitNormalization::InvalidArgument(violation) => {
                                        SelectionOutcome::InvalidArgument(violation)
                                    }
                                    TraitNormalization::Unsatisfied(obligation)
                                    | TraitNormalization::Ambiguous(obligation) => {
                                        SelectionOutcome::Unsatisfied(obligation)
                                    }
                                    TraitNormalization::Cyclic(projection) => {
                                        SelectionOutcome::Unsatisfied(TraitObligation::new(
                                            projection.base(),
                                            projection.trait_ref().clone(),
                                        ))
                                    }
                                    TraitNormalization::Normalized(_)
                                    | TraitNormalization::Symbolic(_) => {
                                        unreachable!("normalization success has an obligation")
                                    }
                                });
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

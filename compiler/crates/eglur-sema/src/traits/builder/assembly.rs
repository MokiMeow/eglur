use super::*;

#[cfg(test)]
mod tests;

pub(super) fn collect_implementations(
    signatures: &DeclarationSignatures,
) -> Result<CollectedImplementations, TraitEnvironmentError> {
    let mut implementations = BTreeMap::new();
    for (declaration, fact) in signatures.impl_headers() {
        let SemanticFact::Known(header) = fact else {
            continue;
        };
        let (bindings, poison) = match signatures.declaration(declaration) {
            FactState::Known(DeclarationSignature::Impl {
                associated_bindings,
            }) => (
                associated_bindings
                    .iter()
                    .cloned()
                    .map(|binding| (binding.associated().clone(), binding))
                    .collect(),
                None,
            ),
            FactState::Poisoned(poison) => (BTreeMap::new(), Some(poison.clone())),
            FactState::Unperformed => return Ok(CollectedImplementations::Unperformed),
            FactState::Known(_) | FactState::NotApplicable => {
                return Err(TraitEnvironmentError::UnexpectedImplSignature(
                    declaration.clone(),
                ));
            }
        };
        implementations.insert(
            declaration.clone(),
            TraitImpl {
                header: header.clone(),
                bindings,
                poison,
            },
        );
    }
    for (declaration, header) in signatures.rejected_impl_headers() {
        let FactState::Poisoned(poison) = signatures.impl_header(declaration) else {
            return Err(TraitEnvironmentError::UnexpectedImplSignature(
                declaration.clone(),
            ));
        };
        implementations.insert(
            declaration.clone(),
            TraitImpl {
                header: header.clone(),
                bindings: BTreeMap::new(),
                poison: Some(poison.clone()),
            },
        );
    }
    Ok(CollectedImplementations::Complete(implementations))
}

// Signature resolution records source diagnostics before it inspects associated bindings. This
// phase accepts that checked state and rejects any unpoisoned overlap as an internal contract bug.
pub(super) fn validate_checked_coherence(
    signatures: &DeclarationSignatures,
    implementations: &BTreeMap<CanonicalTarget, TraitImpl>,
) -> Result<(), TraitEnvironmentError> {
    let mut headers = CoherenceIndex::default();
    for implementation in implementations
        .values()
        .filter(|value| value.poison.is_none())
    {
        headers.insert(signatures.types(), implementation.header.clone())?;
    }
    for (index, left) in headers.headers().iter().enumerate() {
        for (next, right) in headers.candidates(signatures.types(), left)? {
            if next > index && classify_overlap(signatures, left, right)?.is_some() {
                return Err(TraitEnvironmentError::UncheckedCoherence {
                    left: left.declaration().clone(),
                    right: right.declaration().clone(),
                });
            }
        }
    }
    Ok(())
}

pub(super) fn build_buckets(
    types: &CanonicalTypeStore,
    implementations: &BTreeMap<CanonicalTarget, TraitImpl>,
) -> Result<ImplBuckets, TraitEnvironmentError> {
    let mut buckets = BTreeMap::<_, Vec<_>>::new();
    for (declaration, implementation) in implementations {
        let key = (
            implementation.header.trait_ref().declaration().clone(),
            target_bucket(types, implementation.header.target())?,
        );
        buckets.entry(key).or_default().push(declaration.clone());
    }
    Ok(buckets
        .into_iter()
        .map(|(key, mut values)| {
            values.sort();
            values.dedup();
            (key, values.into_boxed_slice())
        })
        .collect())
}

pub(super) fn build_assumptions(
    collection: &DeclarationCollection,
    signatures: &mut DeclarationSignatures,
) -> Result<BTreeMap<DefId, Box<[TraitAssumption]>>, TraitEnvironmentError> {
    let direct_generics = collection
        .modules()
        .flat_map(|module| module.declarations())
        .filter(|declaration| declaration.kind() == DeclarationKind::GenericParameter)
        .filter_map(|declaration| declaration.parent().map(|owner| (owner, declaration.id())))
        .fold(
            BTreeMap::<DefId, Vec<DefId>>::new(),
            |mut by_owner, (owner, generic)| {
                by_owner.entry(owner).or_default().push(generic);
                by_owner
            },
        );
    let declarations = collection
        .modules()
        .flat_map(|module| module.declarations())
        .map(|declaration| declaration.id())
        .collect::<Vec<_>>();
    let mut result = BTreeMap::new();
    for declaration in declarations {
        let mut chain = Vec::new();
        let mut cursor = Some(declaration);
        while let Some(current) = cursor {
            chain.push(current);
            cursor = collection
                .declaration(current)
                .and_then(crate::Declaration::parent);
        }
        chain.reverse();
        let mut assumptions = Vec::new();
        for owner in chain {
            for &generic in direct_generics.get(&owner).map_or(&[][..], Vec::as_slice) {
                let id = GenericParameterId::Source(generic);
                let signature = match signatures.generic_parameter(&id) {
                    FactState::Known(signature) => signature.clone(),
                    FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => {
                        continue;
                    }
                };
                let subject = signatures.types_mut().generic_parameter(id)?;
                assumptions.extend(
                    signature
                        .bounds()
                        .iter()
                        .cloned()
                        .map(|trait_ref| (TraitObligation::new(subject, trait_ref), None)),
                );
            }
            let target = CanonicalTarget::Source(owner);
            match signatures.declaration(&target) {
                FactState::Known(DeclarationSignature::Trait {
                    generic_parameters, ..
                }) => {
                    let generic_parameters = generic_parameters.to_vec();
                    let arguments = generic_parameters
                        .into_iter()
                        .map(|parameter| signatures.types_mut().generic_parameter(parameter))
                        .collect::<Result<Vec<_>, _>>()?;
                    let substitution = signatures.types_mut().intern_substitution(arguments)?;
                    let subject = signatures.types_mut().self_type(target.clone())?;
                    assumptions.push((
                        TraitObligation::new(subject, TraitRef::new(target, substitution)),
                        None,
                    ));
                }
                FactState::Known(DeclarationSignature::Impl { .. }) => {
                    if let FactState::Known(header) = signatures.impl_header(&target) {
                        assumptions.push((
                            TraitObligation::new(header.target(), header.trait_ref().clone()),
                            Some(target),
                        ));
                    }
                }
                FactState::Known(_)
                | FactState::Poisoned(_)
                | FactState::Unperformed
                | FactState::NotApplicable => {}
            }
        }
        let mut by_obligation = BTreeMap::new();
        for (obligation, candidate_evidence) in assumptions {
            by_obligation
                .entry(obligation)
                .and_modify(|existing: &mut Option<CanonicalTarget>| {
                    if existing.is_none() {
                        *existing = candidate_evidence.clone();
                    }
                })
                .or_insert(candidate_evidence);
        }
        let assumptions = by_obligation
            .into_iter()
            .enumerate()
            .map(|(index, (obligation, implementation))| {
                let evidence = match implementation {
                    Some(implementation) => TraitAssumptionEvidence::Implementation(implementation),
                    None => TraitAssumptionEvidence::Witness(WitnessSlotId::from_index(
                        declaration,
                        index,
                    )?),
                };
                Ok(TraitAssumption {
                    obligation,
                    evidence,
                })
            })
            .collect::<Result<Vec<_>, IdentityOverflow>>()?;
        result.insert(declaration, assumptions.into_boxed_slice());
    }
    Ok(result)
}

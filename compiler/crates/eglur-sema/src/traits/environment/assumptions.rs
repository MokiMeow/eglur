use super::*;

pub(super) enum AssumptionNormalization {
    Complete(Vec<TraitAssumption>),
    Incomplete {
        resolved: Vec<TraitAssumption>,
        failure: TraitNormalization,
    },
}

impl AssumptionNormalization {
    pub(super) fn resolved(&self) -> &[TraitAssumption] {
        match self {
            Self::Complete(assumptions) => assumptions,
            Self::Incomplete { resolved, .. } => resolved,
        }
    }

    pub(super) fn into_complete(self) -> Result<Vec<TraitAssumption>, TraitNormalization> {
        match self {
            Self::Complete(assumptions) => Ok(assumptions),
            Self::Incomplete { failure, .. } => Err(failure),
        }
    }
}

impl TraitEnvironment {
    pub(super) fn normalized_owner_assumptions(
        &mut self,
        owner: DefId,
        signatures: &mut DeclarationSignatures,
    ) -> Result<Result<Vec<TraitAssumption>, TraitNormalization>, TraitEnvironmentError> {
        if let FactState::Poisoned(poison) = signatures.source_declaration(owner) {
            return Ok(Err(TraitNormalization::Poisoned(poison.clone())));
        }
        if let Some(assumptions) = self.owner_assumption_cache.get(&owner) {
            return Ok(Ok(assumptions.clone()));
        }
        let declared = self
            .assumptions
            .get(&owner)
            .map_or_else(Vec::new, |values| values.to_vec());
        let result = self
            .normalize_assumptions(signatures, declared)?
            .into_complete();
        if let Ok(assumptions) = &result {
            self.owner_assumption_cache
                .insert(owner, assumptions.clone());
        }
        Ok(result)
    }

    pub(super) fn normalize_assumptions(
        &mut self,
        signatures: &mut DeclarationSignatures,
        assumptions: Vec<TraitAssumption>,
    ) -> Result<AssumptionNormalization, TraitEnvironmentError> {
        let previous_cache = self.local_query_cache.take();
        self.local_query_cache = Some(LocalQueryCache::default());
        let result = self.normalize_assumptions_inner(signatures, assumptions);
        self.local_query_cache = previous_cache;
        result
    }

    fn normalize_assumptions_inner(
        &mut self,
        signatures: &mut DeclarationSignatures,
        mut assumptions: Vec<TraitAssumption>,
    ) -> Result<AssumptionNormalization, TraitEnvironmentError> {
        let mut pending = (0..assumptions.len()).collect::<Vec<_>>();
        while !pending.is_empty() {
            let mut remaining = Vec::new();
            for &index in &pending {
                match self.normalize_obligation(
                    None,
                    signatures,
                    &assumptions[index].obligation,
                    &assumptions,
                    &mut BTreeSet::new(),
                )? {
                    Ok(obligation) => {
                        if assumptions[index].obligation != obligation {
                            assumptions[index].obligation = obligation;
                            // Proof keys are valid only for one exact assumption set.
                            self.local_query_cache = Some(LocalQueryCache::default());
                        }
                    }
                    Err(failure) => remaining.push((index, failure)),
                }
            }
            if remaining.len() == pending.len() {
                let failure = remaining.remove(0).1;
                let resolved = assumptions
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, assumption)| {
                        pending.binary_search(&index).is_err().then_some(assumption)
                    })
                    .collect();
                return Ok(AssumptionNormalization::Incomplete { resolved, failure });
            }
            pending = remaining.into_iter().map(|(index, _)| index).collect();
        }
        Ok(AssumptionNormalization::Complete(assumptions))
    }

    pub(super) fn normalize_obligation(
        &mut self,
        owner: Option<DefId>,
        signatures: &mut DeclarationSignatures,
        obligation: &TraitObligation,
        assumptions: &[TraitAssumption],
        active: &mut BTreeSet<TraitObligation>,
    ) -> Result<Result<TraitObligation, TraitNormalization>, TraitEnvironmentError> {
        let mut components = vec![obligation.subject()];
        components.extend_from_slice(
            signatures
                .types()
                .substitution(obligation.trait_ref().substitution())?,
        );
        let normalized = self.normalize_many(
            owner,
            signatures,
            &components,
            assumptions,
            &mut BTreeSet::new(),
            active,
        )?;
        let (components, _) = match normalized {
            Ok(components) => components,
            Err(failure) => return Ok(Err(failure)),
        };
        let substitution = signatures
            .types_mut()
            .intern_substitution(components[1..].to_vec())?;
        Ok(Ok(TraitObligation::new(
            components[0],
            TraitRef::new(obligation.trait_ref().declaration().clone(), substitution),
        )))
    }
}

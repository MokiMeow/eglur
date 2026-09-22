use super::*;
use crate::signatures::{TypeApplication, TypeApplicationSite};
use crate::{DeclarationSignature, SemanticFact};

pub(in crate::traits) enum ApplicationCheck {
    Checked,
    Failed(TraitNormalization),
    Unperformed,
}

impl TraitEnvironment {
    pub(in crate::traits) fn check_type_application(
        &mut self,
        signatures: &mut DeclarationSignatures,
        application: &TypeApplication,
    ) -> Result<ApplicationCheck, TraitEnvironmentError> {
        self.local_query_cache = Some(LocalQueryCache::default());
        let result = self.check_type_application_inner(signatures, application);
        self.local_query_cache = None;
        result
    }

    fn check_type_application_inner(
        &mut self,
        signatures: &mut DeclarationSignatures,
        application: &TypeApplication,
    ) -> Result<ApplicationCheck, TraitEnvironmentError> {
        let mut assumptions = self.application_assumptions(signatures, application.site())?;
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
                            self.local_query_cache = Some(LocalQueryCache::default());
                        }
                    }
                    Err(_) => remaining.push(index),
                }
            }
            let progressed = remaining.len() < pending.len();
            pending = remaining;
            if !progressed {
                break;
            }
        }
        let assumptions_complete = pending.is_empty();
        let normalized_assumptions = assumptions
            .into_iter()
            .enumerate()
            .filter_map(|(index, assumption)| {
                pending.binary_search(&index).is_err().then_some(assumption)
            })
            .collect::<Vec<_>>();

        // Query keys are scoped to one exact assumption environment, including normalization.
        self.local_query_cache = Some(LocalQueryCache::default());

        let arguments = match self.normalize_many(
            None,
            signatures,
            application.arguments(),
            &normalized_assumptions,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )? {
            Ok((arguments, _)) => arguments,
            Err(failure) => return Ok(ApplicationCheck::Failed(failure)),
        };
        // The offending bound application owns an unavailable environment's diagnostic.
        if !assumptions_complete {
            return Ok(ApplicationCheck::Unperformed);
        }
        if let Some(violation) =
            ImplArgumentViolation::in_arguments(signatures.types(), &arguments)?
        {
            return Ok(ApplicationCheck::Failed(
                TraitNormalization::InvalidArgument(violation),
            ));
        }
        let obligations = match signatures.application_obligations(application, &arguments)? {
            SemanticFact::Known(obligations) => obligations,
            SemanticFact::Poisoned(poison) => {
                return Ok(ApplicationCheck::Failed(TraitNormalization::Poisoned(
                    poison,
                )));
            }
            SemanticFact::NotApplicable => return Ok(ApplicationCheck::Unperformed),
        };
        for obligation in obligations {
            let obligation = match self.normalize_obligation(
                None,
                signatures,
                &obligation,
                &normalized_assumptions,
                &mut BTreeSet::new(),
            )? {
                Ok(obligation) => obligation,
                Err(failure) => return Ok(ApplicationCheck::Failed(failure)),
            };
            match self.select_impl(
                None,
                signatures,
                obligation,
                &normalized_assumptions,
                &mut BTreeSet::new(),
            )? {
                SelectionOutcome::Assumed(_)
                | SelectionOutcome::TrustedBound
                | SelectionOutcome::Selected(_) => {}
                SelectionOutcome::Poisoned(poison) => {
                    return Ok(ApplicationCheck::Failed(TraitNormalization::Poisoned(
                        poison,
                    )));
                }
                SelectionOutcome::InvalidArgument(violation) => {
                    return Ok(ApplicationCheck::Failed(
                        TraitNormalization::InvalidArgument(violation),
                    ));
                }
                SelectionOutcome::Unsatisfied(obligation)
                | SelectionOutcome::Ambiguous(obligation) => {
                    return Ok(ApplicationCheck::Failed(TraitNormalization::Unsatisfied(
                        obligation,
                    )));
                }
            }
        }
        Ok(ApplicationCheck::Checked)
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

    fn application_assumptions(
        &self,
        signatures: &mut DeclarationSignatures,
        site: &TypeApplicationSite,
    ) -> Result<Vec<TraitAssumption>, TraitEnvironmentError> {
        if let TypeApplicationSite::Source { owner, .. } = site {
            return Ok(self
                .assumptions
                .get(owner)
                .map_or_else(Vec::new, |values| values.to_vec()));
        }
        let owner = site.owner();
        let mut assumptions = Vec::new();
        let parameters = match signatures.type_parameters(&owner) {
            FactState::Known(parameters) => parameters.to_vec(),
            _ => Vec::new(),
        };
        for parameter in parameters {
            let bounds = match signatures.generic_parameter(&parameter) {
                FactState::Known(parameter) => parameter.bounds().to_vec(),
                _ => {
                    return Err(TraitEnvironmentError::UnavailableGenericParameter(
                        parameter,
                    ));
                }
            };
            let subject = signatures.types_mut().generic_parameter(parameter)?;
            assumptions.extend(bounds.into_iter().map(|bound| TraitAssumption {
                obligation: TraitObligation::new(subject, bound),
                evidence: TraitAssumptionEvidence::Trusted,
            }));
        }
        if let Some(implementation) = self.implementations.get(&owner) {
            assumptions.push(TraitAssumption {
                obligation: TraitObligation::new(
                    implementation.header.target(),
                    implementation.header.trait_ref().clone(),
                ),
                evidence: TraitAssumptionEvidence::Implementation(owner.clone()),
            });
        }
        let trait_owner = signatures.trait_method_owner(&owner).cloned().or_else(|| {
            matches!(
                signatures.declaration(&owner),
                FactState::Known(DeclarationSignature::Trait { .. })
            )
            .then_some(owner)
        });
        if let Some(trait_owner) = trait_owner {
            let parameters = match signatures.type_parameters(&trait_owner) {
                FactState::Known(parameters) => parameters.to_vec(),
                _ => Vec::new(),
            };
            let arguments = parameters
                .into_iter()
                .map(|parameter| signatures.types_mut().generic_parameter(parameter))
                .collect::<Result<Vec<_>, _>>()?;
            let substitution = signatures.types_mut().intern_substitution(arguments)?;
            let subject = signatures.types_mut().self_type(trait_owner.clone())?;
            assumptions.push(TraitAssumption {
                obligation: TraitObligation::new(subject, TraitRef::new(trait_owner, substitution)),
                evidence: TraitAssumptionEvidence::Trusted,
            });
        }
        Ok(assumptions)
    }
}

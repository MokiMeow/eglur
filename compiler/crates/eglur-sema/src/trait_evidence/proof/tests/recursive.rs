use std::collections::BTreeMap;

use crate::{
    DeclarationSignature, DeclarationSignatures, DefId, EvidenceProofId, FactState,
    GenericParameterId, TraitEnvironment, TraitImplEvidence, TraitObligation, TraitProof, TypeId,
};

use super::{EvidenceSource, ProofArena, TraitEvidenceError};

impl ProofArena {
    pub(super) fn recursive_materialize(
        &mut self,
        owner: DefId,
        obligation: TraitObligation,
        selected: TraitProof,
        signatures: &mut DeclarationSignatures,
        environment: &mut TraitEnvironment,
    ) -> Result<EvidenceProofId, TraitEvidenceError> {
        if let Some(&id) = self.by_obligation.get(&(owner, obligation.clone())) {
            return Ok(id);
        }
        if !self.active.insert((owner, obligation.clone())) {
            return Err(TraitEvidenceError::CyclicCheckedProof(obligation));
        }

        let node = match selected {
            TraitProof::Bound(slot) => EvidenceSource::Bound(slot),
            TraitProof::Implementation(evidence) => {
                self.recursive_implementation(owner, &evidence, signatures, environment)?
            }
            TraitProof::Poisoned(_)
            | TraitProof::InvalidArgument(_)
            | TraitProof::Unsatisfied(_)
            | TraitProof::Ambiguous(_) => {
                return Err(TraitEvidenceError::UnmaterializablePrerequisite(obligation));
            }
        };

        self.active.remove(&(owner, obligation.clone()));
        let id = self.intern(node)?;
        self.by_obligation.insert((owner, obligation), id);
        Ok(id)
    }

    fn recursive_implementation(
        &mut self,
        owner: DefId,
        selected: &TraitImplEvidence,
        signatures: &mut DeclarationSignatures,
        environment: &mut TraitEnvironment,
    ) -> Result<EvidenceSource, TraitEvidenceError> {
        let (header, _) = environment
            .checked_implementation_contract(selected.implementation())
            .ok_or_else(|| {
                TraitEvidenceError::UnknownCheckedImplementation(selected.implementation().clone())
            })?;
        let header = header.clone();
        let arguments = signatures
            .types()
            .substitution(selected.substitution())
            .map_err(crate::TraitEnvironmentError::from)?
            .to_vec();
        if header.generic_parameters().len() != arguments.len() {
            return Err(TraitEvidenceError::InvalidCheckedProof(
                "implementation substitution arity",
            ));
        }
        let replacements = header
            .generic_parameters()
            .iter()
            .cloned()
            .zip(arguments)
            .collect::<BTreeMap<_, _>>();
        let implementation_bounds = self.recursive_parameter_bounds(
            owner,
            header.generic_parameters(),
            &replacements,
            signatures,
            environment,
        )?;

        let trait_arguments = signatures
            .types()
            .substitution(header.trait_ref().substitution())
            .map_err(crate::TraitEnvironmentError::from)?
            .to_vec();
        let trait_arguments = trait_arguments
            .into_iter()
            .map(|argument| {
                signatures
                    .types_mut()
                    .substitute(argument, &replacements)
                    .map_err(crate::TraitEnvironmentError::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let trait_parameters = match signatures.declaration(header.trait_ref().declaration()) {
            FactState::Known(DeclarationSignature::Trait {
                generic_parameters, ..
            }) => generic_parameters.to_vec(),
            _ => {
                return Err(TraitEvidenceError::InvalidCheckedProof(
                    "selected trait signature",
                ));
            }
        };
        if trait_parameters.len() != trait_arguments.len() {
            return Err(TraitEvidenceError::InvalidCheckedProof(
                "trait substitution arity",
            ));
        }
        let trait_replacements = trait_parameters
            .iter()
            .cloned()
            .zip(trait_arguments)
            .collect::<BTreeMap<_, _>>();
        let trait_bounds = self.recursive_parameter_bounds(
            owner,
            &trait_parameters,
            &trait_replacements,
            signatures,
            environment,
        )?;

        Ok(EvidenceSource::Implementation {
            implementation: selected.implementation().clone(),
            substitution: selected.substitution(),
            implementation_bounds,
            trait_bounds,
        })
    }

    fn recursive_parameter_bounds(
        &mut self,
        owner: DefId,
        parameters: &[GenericParameterId],
        replacements: &BTreeMap<GenericParameterId, TypeId>,
        signatures: &mut DeclarationSignatures,
        environment: &mut TraitEnvironment,
    ) -> Result<Box<[EvidenceProofId]>, TraitEvidenceError> {
        let mut proofs = Vec::new();
        for parameter in parameters {
            let signature = match signatures.generic_parameter(parameter) {
                FactState::Known(signature) => signature.clone(),
                _ => return Err(TraitEvidenceError::InvalidCheckedProof("generic signature")),
            };
            let bounds = signature
                .instantiate_bounds(signatures.types_mut(), replacements)
                .map_err(crate::TraitEnvironmentError::from)?;
            for bound in bounds {
                let obligation = bound.into_obligation();
                let selected =
                    environment.prove_for_owner(owner, signatures, obligation.clone())?;
                proofs.push(self.recursive_materialize(
                    owner,
                    obligation,
                    selected,
                    signatures,
                    environment,
                )?);
            }
        }
        Ok(proofs.into_boxed_slice())
    }
}

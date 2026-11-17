use std::collections::BTreeMap;

use crate::{
    DeclarationSignature, DeclarationSignatures, EvidenceProofId, FactState, GenericParameterId,
    TraitEnvironment, TraitImplEvidence, TraitImplHeader, TraitObligation, TypeId,
    signatures::InstantiatedParameterBound,
};

use super::{EvidenceSource, TraitEvidenceError};

pub(super) enum ImplementationStep {
    Prerequisite(TraitObligation),
    Complete(EvidenceSource),
}

pub(super) struct ImplementationFrame {
    selected: TraitImplEvidence,
    header: TraitImplHeader,
    replacements: BTreeMap<GenericParameterId, TypeId>,
    phase: BoundPhase,
}

enum BoundPhase {
    Implementation(BoundCursor),
    Trait {
        implementation_bounds: Box<[EvidenceProofId]>,
        parameters: Vec<GenericParameterId>,
        replacements: BTreeMap<GenericParameterId, TypeId>,
        cursor: BoundCursor,
    },
}

#[derive(Default)]
struct BoundCursor {
    parameter: usize,
    pending: std::vec::IntoIter<InstantiatedParameterBound>,
    proofs: Vec<EvidenceProofId>,
}

impl ImplementationFrame {
    pub(super) fn new(
        selected: TraitImplEvidence,
        signatures: &DeclarationSignatures,
        environment: &TraitEnvironment,
    ) -> Result<Self, TraitEvidenceError> {
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
            .collect();

        Ok(Self {
            selected,
            header,
            replacements,
            phase: BoundPhase::Implementation(BoundCursor::default()),
        })
    }

    pub(super) fn accept(&mut self, proof: EvidenceProofId) {
        match &mut self.phase {
            BoundPhase::Implementation(cursor) | BoundPhase::Trait { cursor, .. } => {
                cursor.proofs.push(proof);
            }
        }
    }

    pub(super) fn advance(
        &mut self,
        signatures: &mut DeclarationSignatures,
    ) -> Result<ImplementationStep, TraitEvidenceError> {
        loop {
            match &mut self.phase {
                BoundPhase::Implementation(cursor) => {
                    if let Some(obligation) = cursor.next(
                        self.header.generic_parameters(),
                        &self.replacements,
                        signatures,
                    )? {
                        return Ok(ImplementationStep::Prerequisite(obligation));
                    }
                    let proofs = std::mem::take(&mut cursor.proofs).into_boxed_slice();
                    self.phase = self.trait_phase(proofs, signatures)?;
                }
                BoundPhase::Trait {
                    implementation_bounds,
                    parameters,
                    replacements,
                    cursor,
                } => {
                    if let Some(obligation) = cursor.next(parameters, replacements, signatures)? {
                        return Ok(ImplementationStep::Prerequisite(obligation));
                    }
                    return Ok(ImplementationStep::Complete(
                        EvidenceSource::Implementation {
                            implementation: self.selected.implementation().clone(),
                            substitution: self.selected.substitution(),
                            implementation_bounds: std::mem::take(implementation_bounds),
                            trait_bounds: std::mem::take(&mut cursor.proofs).into_boxed_slice(),
                        },
                    ));
                }
            }
        }
    }

    fn trait_phase(
        &self,
        implementation_bounds: Box<[EvidenceProofId]>,
        signatures: &mut DeclarationSignatures,
    ) -> Result<BoundPhase, TraitEvidenceError> {
        let trait_arguments = signatures
            .types()
            .substitution(self.header.trait_ref().substitution())
            .map_err(crate::TraitEnvironmentError::from)?
            .to_vec();
        let trait_arguments = trait_arguments
            .into_iter()
            .map(|argument| {
                signatures
                    .types_mut()
                    .substitute(argument, &self.replacements)
                    .map_err(crate::TraitEnvironmentError::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let parameters = match signatures.declaration(self.header.trait_ref().declaration()) {
            FactState::Known(DeclarationSignature::Trait {
                generic_parameters, ..
            }) => generic_parameters.to_vec(),
            _ => {
                return Err(TraitEvidenceError::InvalidCheckedProof(
                    "selected trait signature",
                ));
            }
        };
        if parameters.len() != trait_arguments.len() {
            return Err(TraitEvidenceError::InvalidCheckedProof(
                "trait substitution arity",
            ));
        }
        let replacements = parameters.iter().cloned().zip(trait_arguments).collect();
        Ok(BoundPhase::Trait {
            implementation_bounds,
            parameters,
            replacements,
            cursor: BoundCursor::default(),
        })
    }
}

impl BoundCursor {
    fn next(
        &mut self,
        parameters: &[GenericParameterId],
        replacements: &BTreeMap<GenericParameterId, TypeId>,
        signatures: &mut DeclarationSignatures,
    ) -> Result<Option<TraitObligation>, TraitEvidenceError> {
        loop {
            if let Some(bound) = self.pending.next() {
                return Ok(Some(bound.into_obligation()));
            }
            let Some(parameter) = parameters.get(self.parameter) else {
                return Ok(None);
            };
            let signature = match signatures.generic_parameter(parameter) {
                FactState::Known(signature) => signature.clone(),
                _ => return Err(TraitEvidenceError::InvalidCheckedProof("generic signature")),
            };
            self.pending = signature
                .instantiate_bounds(signatures.types_mut(), replacements)
                .map_err(crate::TraitEnvironmentError::from)?
                .into_iter();
            self.parameter += 1;
        }
    }
}

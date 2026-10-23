use std::collections::{BTreeMap, BTreeSet};

use crate::record_reservation::RecordReservation;
use crate::{
    DeclarationSignatures, DefId, EvidenceProofId, TraitEnvironment, TraitObligation, TraitProof,
};

use super::{EvidenceSource, TraitEvidenceError};
use super::{execution::EvidenceFlow, work::ProofWork};

mod implementation;

use implementation::{ImplementationFrame, ImplementationStep};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod record_growth_tests;

#[derive(Default)]
pub(super) struct ProofArena {
    nodes: Vec<EvidenceSource>,
    ids: BTreeMap<EvidenceSource, EvidenceProofId>,
    by_obligation: BTreeMap<(DefId, TraitObligation), EvidenceProofId>,
    active: BTreeSet<(DefId, TraitObligation)>,
}

impl ProofArena {
    pub(super) fn into_nodes(self) -> Box<[EvidenceSource]> {
        self.nodes.into_boxed_slice()
    }

    pub(super) fn into_completed_nodes(self) -> Vec<EvidenceSource> {
        self.nodes
    }

    pub(super) fn materialize(
        &mut self,
        owner: DefId,
        obligation: TraitObligation,
        selected: TraitProof,
        signatures: &mut DeclarationSignatures,
        environment: &mut TraitEnvironment,
        work: &mut ProofWork<'_>,
    ) -> Result<EvidenceProofId, EvidenceFlow> {
        let mut pending = Some((obligation, selected));
        let mut completed = None;
        let mut frames: Vec<(TraitObligation, ImplementationFrame)> = Vec::new();

        loop {
            #[cfg(test)]
            work.observe(super::execution::tests::ProofStage::Materialization);
            work.checkpoint()?;
            if let Some((obligation, selected)) = pending.take() {
                if let Some(&id) = self.by_obligation.get(&(owner, obligation.clone())) {
                    completed = Some(id);
                } else {
                    if !self.active.insert((owner, obligation.clone())) {
                        return Err(TraitEvidenceError::CyclicCheckedProof(obligation).into());
                    }

                    match selected {
                        TraitProof::Bound(slot) => {
                            completed = Some(self.finish(
                                owner,
                                obligation,
                                EvidenceSource::Bound(slot),
                            )?);
                        }
                        TraitProof::Implementation(evidence) => frames.push((
                            obligation,
                            ImplementationFrame::new(evidence, signatures, environment, work)?,
                        )),
                        TraitProof::Poisoned(_)
                        | TraitProof::InvalidArgument(_)
                        | TraitProof::Unsatisfied(_)
                        | TraitProof::Ambiguous(_) => {
                            return Err(TraitEvidenceError::UnmaterializablePrerequisite(
                                obligation,
                            )
                            .into());
                        }
                    }
                }
            }

            if let Some(id) = completed.take() {
                let Some((_, frame)) = frames.last_mut() else {
                    return Ok(id);
                };
                frame.accept(id);
            }

            match frames
                .last_mut()
                .expect("an unfinished implementation has a frame")
                .1
                .advance(signatures, work)?
            {
                ImplementationStep::Prerequisite(obligation) => {
                    let selected =
                        environment.prove_for_owner(owner, signatures, obligation.clone())?;
                    pending = Some((obligation, selected));
                }
                ImplementationStep::Complete(node) => {
                    let (obligation, _) = frames.pop().unwrap();
                    completed = Some(self.finish(owner, obligation, node)?);
                }
            }
        }
    }

    fn finish(
        &mut self,
        owner: DefId,
        obligation: TraitObligation,
        node: EvidenceSource,
    ) -> Result<EvidenceProofId, TraitEvidenceError> {
        self.active.remove(&(owner, obligation.clone()));
        let id = self.intern(node)?;
        self.by_obligation.insert((owner, obligation), id);
        Ok(id)
    }

    fn intern(&mut self, node: EvidenceSource) -> Result<EvidenceProofId, TraitEvidenceError> {
        if let Some(&id) = self.ids.get(&node) {
            return Ok(id);
        }
        let id = EvidenceProofId::from_index(self.nodes.len())?;

        RecordReservation::for_append(&self.nodes).reserve(&mut self.nodes);

        self.ids.insert(node.clone(), id);
        self.nodes.push(node);
        Ok(id)
    }
}

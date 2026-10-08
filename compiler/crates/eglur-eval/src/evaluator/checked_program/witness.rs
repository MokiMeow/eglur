use super::{WitnessId, WitnessStore, WitnessValue, Witnesses};
use eglur_sema::{CanonicalTarget, EvidenceProofId, EvidenceSource, SubstId};
use std::collections::{BTreeMap, BTreeSet};
use std::slice;

#[cfg(test)]
mod tests;

struct WitnessFrame<'a> {
    proof: EvidenceProofId,
    implementation: &'a CanonicalTarget,
    substitution: SubstId,
    stage: WitnessStage<'a>,
}

enum WitnessStage<'a> {
    Implementation {
        remaining: slice::Iter<'a, EvidenceProofId>,
        trait_bounds: &'a [EvidenceProofId],
        completed: Vec<WitnessId>,
    },
    Trait {
        remaining: slice::Iter<'a, EvidenceProofId>,
        implementation_bounds: Box<[WitnessId]>,
        completed: Vec<WitnessId>,
    },
}

enum WitnessStep {
    Child(EvidenceProofId),
    Complete,
}

impl<'a> WitnessFrame<'a> {
    fn new(
        proof: EvidenceProofId,
        implementation: &'a CanonicalTarget,
        substitution: SubstId,
        implementation_bounds: &'a [EvidenceProofId],
        trait_bounds: &'a [EvidenceProofId],
    ) -> Self {
        Self {
            proof,
            implementation,
            substitution,
            stage: WitnessStage::Implementation {
                remaining: implementation_bounds.iter(),
                trait_bounds,
                completed: Vec::new(),
            },
        }
    }

    fn accept_child(&mut self, id: WitnessId) {
        match &mut self.stage {
            WitnessStage::Implementation { completed, .. }
            | WitnessStage::Trait { completed, .. } => completed.push(id),
        }
    }

    fn advance(&mut self) -> WitnessStep {
        loop {
            match &mut self.stage {
                WitnessStage::Implementation {
                    remaining,
                    trait_bounds,
                    completed,
                } => {
                    if let Some(&proof) = remaining.next() {
                        return WitnessStep::Child(proof);
                    }
                    self.stage = WitnessStage::Trait {
                        remaining: trait_bounds.iter(),
                        implementation_bounds: std::mem::take(completed).into_boxed_slice(),
                        completed: Vec::new(),
                    };
                }
                WitnessStage::Trait { remaining, .. } => {
                    return remaining
                        .next()
                        .map_or(WitnessStep::Complete, |&proof| WitnessStep::Child(proof));
                }
            }
        }
    }

    fn into_value(self) -> WitnessValue {
        let WitnessStage::Trait {
            implementation_bounds,
            completed,
            ..
        } = self.stage
        else {
            unreachable!("a completed witness has visited both bound families")
        };
        WitnessValue {
            implementation: self.implementation.clone(),
            substitution: self.substitution,
            implementation_bounds,
            trait_bounds: completed.into_boxed_slice(),
        }
    }
}

impl WitnessStore {
    pub(super) fn resolve_inner<'evidence>(
        &mut self,
        lookup: &impl Fn(EvidenceProofId) -> Option<&'evidence EvidenceSource>,
        proof: EvidenceProofId,
        parent: &Witnesses,
        cache: &mut BTreeMap<EvidenceProofId, WitnessId>,
        active: &mut BTreeSet<EvidenceProofId>,
    ) -> Option<WitnessId> {
        let mut pending = Some(proof);
        let mut completed = None;
        let mut frames: Vec<WitnessFrame<'_>> = Vec::new();

        loop {
            if let Some(proof) = pending.take() {
                if let Some(&id) = cache.get(&proof) {
                    completed = Some(id);
                } else {
                    if !active.insert(proof) {
                        return None;
                    }
                    match lookup(proof)? {
                        EvidenceSource::Bound(slot) => {
                            let id = *parent.get(slot)?;
                            active.remove(&proof);
                            cache.insert(proof, id);
                            completed = Some(id);
                        }
                        EvidenceSource::Implementation {
                            implementation,
                            substitution,
                            implementation_bounds,
                            trait_bounds,
                        } => frames.push(WitnessFrame::new(
                            proof,
                            implementation,
                            *substitution,
                            implementation_bounds,
                            trait_bounds,
                        )),
                    }
                }
            }

            if let Some(id) = completed.take() {
                let Some(frame) = frames.last_mut() else {
                    return Some(id);
                };
                frame.accept_child(id);
            }

            match frames
                .last_mut()
                .expect("an unfinished witness has a frame")
                .advance()
            {
                WitnessStep::Child(proof) => pending = Some(proof),
                WitnessStep::Complete => {
                    let frame = frames.pop().unwrap();
                    let proof = frame.proof;
                    let id = self.intern(frame.into_value());
                    active.remove(&proof);
                    cache.insert(proof, id);
                    completed = Some(id);
                }
            }
        }
    }
}

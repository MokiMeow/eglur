use super::*;

impl WitnessStore {
    pub(super) fn recursive_resolve<'evidence>(
        &mut self,
        lookup: &impl Fn(EvidenceProofId) -> Option<&'evidence EvidenceSource>,
        proof: EvidenceProofId,
        parent: &Witnesses,
        cache: &mut BTreeMap<EvidenceProofId, WitnessId>,
        active: &mut BTreeSet<EvidenceProofId>,
    ) -> Option<WitnessId> {
        if let Some(&id) = cache.get(&proof) {
            return Some(id);
        }
        if !active.insert(proof) {
            return None;
        }
        let id = match lookup(proof)? {
            EvidenceSource::Bound(slot) => *parent.get(slot)?,
            EvidenceSource::Implementation {
                implementation,
                substitution,
                implementation_bounds,
                trait_bounds,
            } => {
                let mut resolve = |bounds: &[EvidenceProofId]| {
                    bounds
                        .iter()
                        .map(|&child| self.recursive_resolve(lookup, child, parent, cache, active))
                        .collect::<Option<Vec<_>>>()
                        .map(Vec::into_boxed_slice)
                };
                let implementation_bounds = resolve(implementation_bounds)?;
                let trait_bounds = resolve(trait_bounds)?;
                self.intern(WitnessValue {
                    implementation: implementation.clone(),
                    substitution: *substitution,
                    implementation_bounds,
                    trait_bounds,
                })
            }
        };
        active.remove(&proof);
        cache.insert(proof, id);
        Some(id)
    }
}

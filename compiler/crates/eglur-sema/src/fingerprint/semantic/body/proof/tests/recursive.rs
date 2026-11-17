use super::*;

impl NodeHasher<'_, '_> {
    pub(super) fn recursive_stable_proof(
        &mut self,
        id: EvidenceProofId,
    ) -> Result<FingerprintDigest, SemanticFingerprintError> {
        if let Some(&digest) = self.proofs.get(&id) {
            return Ok(digest);
        }
        if !self.active_proofs.insert(id) {
            return Err(SemanticFingerprintError::CyclicEvidenceProof);
        }
        let node = self
            .trait_evidence
            .proof(id)
            .ok_or(SemanticFingerprintError::UnknownEvidenceProof)?
            .clone();
        let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
        builder.write_u8(3);
        match node {
            EvidenceSource::Bound(witness) => {
                builder.write_u8(0);
                self.stable_witness(witness)?.write_to(&mut builder);
            }
            EvidenceSource::Implementation {
                implementation,
                substitution,
                implementation_bounds,
                trait_bounds,
            } => {
                builder.write_u8(1);
                self.keys
                    .target(&implementation)
                    .ok_or_else(|| stable_target_error(&implementation))?
                    .write_to(&mut builder);
                self.write_substitution(&mut builder, substitution)?;
                for bounds in [&implementation_bounds, &trait_bounds] {
                    let mut stable_bounds = bounds
                        .iter()
                        .map(|&child| self.recursive_stable_proof(child))
                        .collect::<Result<Vec<_>, _>>()?;
                    stable_bounds.sort();
                    builder.write_len(stable_bounds.len());
                    for child in stable_bounds {
                        builder.write_digest(child);
                    }
                }
            }
        }
        let digest = builder.finish();
        self.active_proofs.remove(&id);
        self.proofs.insert(id, digest);
        Ok(digest)
    }
}

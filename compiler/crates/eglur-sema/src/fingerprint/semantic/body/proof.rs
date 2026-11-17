use super::{
    CanonicalFingerprintBuilder, EvidenceProofId, EvidenceSource, FingerprintDigest,
    FingerprintDomain, NodeHasher, SemanticFingerprintError, stable_target_error,
};

#[cfg(test)]
mod tests;

struct ProofFrame {
    id: EvidenceProofId,
    builder: CanonicalFingerprintBuilder,
    families: [std::vec::IntoIter<EvidenceProofId>; 2],
    family: usize,
    digests: Vec<FingerprintDigest>,
}

enum ProofStep {
    Child(EvidenceProofId),
    Complete,
}

impl ProofFrame {
    fn new(
        id: EvidenceProofId,
        builder: CanonicalFingerprintBuilder,
        implementation_bounds: Box<[EvidenceProofId]>,
        trait_bounds: Box<[EvidenceProofId]>,
    ) -> Self {
        Self {
            id,
            builder,
            families: [
                implementation_bounds.into_vec().into_iter(),
                trait_bounds.into_vec().into_iter(),
            ],
            family: 0,
            digests: Vec::new(),
        }
    }

    fn advance(&mut self) -> ProofStep {
        while self.family < self.families.len() {
            if let Some(child) = self.families[self.family].next() {
                return ProofStep::Child(child);
            }
            self.digests.sort();
            self.builder.write_len(self.digests.len());
            for digest in self.digests.drain(..) {
                self.builder.write_digest(digest);
            }
            self.family += 1;
        }
        ProofStep::Complete
    }
}

impl NodeHasher<'_, '_> {
    pub(super) fn stable_proof(
        &mut self,
        id: EvidenceProofId,
    ) -> Result<FingerprintDigest, SemanticFingerprintError> {
        let mut pending = Some(id);
        let mut completed = None;
        let mut frames: Vec<ProofFrame> = Vec::new();

        loop {
            if let Some(id) = pending.take() {
                if let Some(&digest) = self.proofs.get(&id) {
                    completed = Some(digest);
                } else {
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
                            completed = Some(self.finish_proof(id, builder.finish()));
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
                            frames.push(ProofFrame::new(
                                id,
                                builder,
                                implementation_bounds,
                                trait_bounds,
                            ));
                        }
                    }
                }
            }

            if let Some(digest) = completed.take() {
                let Some(frame) = frames.last_mut() else {
                    return Ok(digest);
                };
                frame.digests.push(digest);
            }

            match frames
                .last_mut()
                .expect("an unfinished proof has a frame")
                .advance()
            {
                ProofStep::Child(id) => pending = Some(id),
                ProofStep::Complete => {
                    let frame = frames.pop().unwrap();
                    completed = Some(self.finish_proof(frame.id, frame.builder.finish()));
                }
            }
        }
    }

    fn finish_proof(
        &mut self,
        id: EvidenceProofId,
        digest: FingerprintDigest,
    ) -> FingerprintDigest {
        self.active_proofs.remove(&id);
        self.proofs.insert(id, digest);
        digest
    }
}

use std::collections::BTreeMap;

use super::overlap::{ImplTargetBucket, target_bucket};
use crate::{CanonicalTarget, CanonicalTypeStore, TraitImplHeader, TypeStoreError};

#[derive(Default)]
pub(crate) struct CoherenceIndex {
    headers: Vec<TraitImplHeader>,
    buckets: BTreeMap<(CanonicalTarget, ImplTargetBucket), Vec<usize>>,
    traits: BTreeMap<CanonicalTarget, Vec<usize>>,
}

impl CoherenceIndex {
    pub(crate) fn insert(
        &mut self,
        types: &CanonicalTypeStore,
        header: TraitImplHeader,
    ) -> Result<(), TypeStoreError> {
        let declaration = header.trait_ref().declaration().clone();
        let bucket = target_bucket(types, header.target())?;
        let index = self.headers.len();
        self.buckets
            .entry((declaration.clone(), bucket))
            .or_default()
            .push(index);
        self.traits.entry(declaration).or_default().push(index);
        self.headers.push(header);
        Ok(())
    }

    pub(crate) fn headers(&self) -> &[TraitImplHeader] {
        &self.headers
    }

    pub(crate) fn candidates<'a>(
        &'a self,
        types: &CanonicalTypeStore,
        header: &TraitImplHeader,
    ) -> Result<impl Iterator<Item = (usize, &'a TraitImplHeader)> + use<'a>, TypeStoreError> {
        let declaration = header.trait_ref().declaration();
        let bucket = target_bucket(types, header.target())?;
        let (rigid, blanket) = if bucket == ImplTargetBucket::Blanket {
            (
                self.traits.get(declaration).map_or(&[][..], Vec::as_slice),
                &[][..],
            )
        } else {
            (
                self.buckets
                    .get(&(declaration.clone(), bucket))
                    .map_or(&[][..], Vec::as_slice),
                self.buckets
                    .get(&(declaration.clone(), ImplTargetBucket::Blanket))
                    .map_or(&[][..], Vec::as_slice),
            )
        };
        let mut rigid = rigid.iter().copied().peekable();
        let mut blanket = blanket.iter().copied().peekable();

        // Bucket order must not replace admission order when selecting the first conflict.
        Ok(std::iter::from_fn(move || {
            let next = match (rigid.peek(), blanket.peek()) {
                (Some(left), Some(right)) if left < right => rigid.next(),
                (Some(_), Some(_)) => blanket.next(),
                (Some(_), None) => rigid.next(),
                (None, Some(_)) => blanket.next(),
                (None, None) => None,
            };
            next.map(|index| (index, &self.headers[index]))
        }))
    }
}

#[cfg(test)]
pub(super) mod tests;

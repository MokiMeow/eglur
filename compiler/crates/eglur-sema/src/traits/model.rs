use std::collections::BTreeMap;

use eglur_diagnostics::Poison;

use crate::{
    AssociatedProjection, CanonicalTarget, CanonicalTypeStore, GenericParameterId, SubstId,
    TraitRef, TypeId, TypeStoreError, WitnessSlotId,
};

/// An inferred impl argument outside the generic parameter domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImplArgumentViolation {
    Ownership(TypeId),
    Reference(TypeId),
}

impl ImplArgumentViolation {
    pub(super) fn in_arguments(
        types: &CanonicalTypeStore,
        arguments: &[TypeId],
    ) -> Result<Option<Self>, TypeStoreError> {
        for &argument in arguments {
            if types.contains_ownership(argument)? {
                return Ok(Some(Self::Ownership(argument)));
            }
        }
        for &argument in arguments {
            if types.contains_reference(argument)? {
                return Ok(Some(Self::Reference(argument)));
            }
        }
        Ok(None)
    }
}

/// One exact nominal trait obligation in a declaration's well-formedness environment.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TraitObligation {
    pub(super) subject: TypeId,
    pub(super) trait_ref: TraitRef,
}

impl TraitObligation {
    #[must_use]
    pub const fn new(subject: TypeId, trait_ref: TraitRef) -> Self {
        Self { subject, trait_ref }
    }

    #[must_use]
    pub const fn subject(&self) -> TypeId {
        self.subject
    }

    #[must_use]
    pub const fn trait_ref(&self) -> &TraitRef {
        &self.trait_ref
    }
}

/// One checked impl selection and its complete declaration-order substitution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitImplEvidence {
    pub(super) implementation: CanonicalTarget,
    pub(super) substitution: SubstId,
}

impl TraitImplEvidence {
    #[must_use]
    pub const fn implementation(&self) -> &CanonicalTarget {
        &self.implementation
    }

    #[must_use]
    pub const fn substitution(&self) -> SubstId {
        self.substitution
    }
}

/// Bounded proof result for one exact nominal trait obligation.
#[derive(Clone, Debug)]
pub enum TraitProof {
    Bound(WitnessSlotId),
    Implementation(TraitImplEvidence),
    Poisoned(Poison),
    InvalidArgument(ImplArgumentViolation),
    Unsatisfied(TraitObligation),
    Ambiguous(TraitObligation),
}

/// Structural receiver match retained until method arguments complete inference.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TraitReceiverCandidate {
    pub(super) trait_ref: TraitRef,
    pub(super) generic_parameters: Box<[GenericParameterId]>,
    pub(super) replacements: BTreeMap<GenericParameterId, TypeId>,
}

impl TraitReceiverCandidate {
    pub(crate) const fn trait_ref(&self) -> &TraitRef {
        &self.trait_ref
    }

    pub(crate) fn generic_parameters(&self) -> &[GenericParameterId] {
        &self.generic_parameters
    }

    pub(crate) const fn replacements(&self) -> &BTreeMap<GenericParameterId, TypeId> {
        &self.replacements
    }
}

/// Result of one bounded normalization query in a source declaration context.
#[derive(Clone, Debug)]
pub enum TraitNormalization {
    Normalized(TypeId),
    Symbolic(TypeId),
    Poisoned(Poison),
    InvalidArgument(ImplArgumentViolation),
    Unsatisfied(TraitObligation),
    Ambiguous(TraitObligation),
    Cyclic(AssociatedProjection),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntrinsicNominal, NominalHead, PrimitiveType, ReferenceMutability};

    #[test]
    fn complete_impl_vector_checks_ownership_before_any_reference() {
        let mut types = CanonicalTypeStore::new().unwrap();
        let integer = types.primitive(PrimitiveType::I64).unwrap();
        let reference = types
            .reference(ReferenceMutability::Shared, integer)
            .unwrap();
        let owned = types
            .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [integer])
            .unwrap();
        for arguments in [[reference, owned], [owned, reference]] {
            assert_eq!(
                ImplArgumentViolation::in_arguments(&types, &arguments).unwrap(),
                Some(ImplArgumentViolation::Ownership(owned))
            );
        }
        let both = types.tuple([reference, owned]).unwrap();
        assert_eq!(
            ImplArgumentViolation::in_arguments(&types, &[both]).unwrap(),
            Some(ImplArgumentViolation::Ownership(both))
        );
        assert_eq!(
            ImplArgumentViolation::in_arguments(&types, &[reference]).unwrap(),
            Some(ImplArgumentViolation::Reference(reference))
        );
        assert_eq!(
            ImplArgumentViolation::in_arguments(&types, &[integer]).unwrap(),
            None
        );
        assert_eq!(
            ImplArgumentViolation::in_arguments(&types, &[]).unwrap(),
            None
        );
    }
}

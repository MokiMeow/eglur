use std::collections::BTreeMap;

use crate::{
    CanonicalTypeStore, GenericParameterId, TraitObligation, TraitRef, TypeId, TypeStoreError,
};

mod records;

pub(crate) use records::GenericParameterRecords;
pub use records::{GenericParameterRecord, GenericParameterRecordId};

/// Resolved parameter bounds with distinct declaration and semantic orders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericParameterSignature {
    id: GenericParameterId,
    name: Box<str>,
    declared_bounds: Box<[TraitRef]>,
    bounds: Box<[TraitRef]>,
}

impl GenericParameterSignature {
    pub(super) fn new(
        id: GenericParameterId,
        name: Box<str>,
        declared_bounds: Vec<TraitRef>,
    ) -> Self {
        let mut bounds = declared_bounds.clone();
        bounds.sort();
        Self {
            id,
            name,
            declared_bounds: declared_bounds.into_boxed_slice(),
            bounds: bounds.into_boxed_slice(),
        }
    }

    #[must_use]
    pub const fn id(&self) -> &GenericParameterId {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Order-insensitive conjunction in canonical order, not diagnostic priority.
    #[must_use]
    pub fn bounds(&self) -> &[TraitRef] {
        &self.bounds
    }

    #[must_use]
    pub(crate) fn declared_bounds(&self) -> &[TraitRef] {
        &self.declared_bounds
    }

    /// Instantiates declaration-ordered bounds using the complete argument substitution.
    pub(crate) fn instantiate_bounds(
        &self,
        types: &mut CanonicalTypeStore,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<Vec<InstantiatedParameterBound>, TypeStoreError> {
        let subject = replacements[&self.id];
        self.declared_bounds
            .iter()
            .map(|declared| {
                InstantiatedParameterBound::instantiate(
                    declared.clone(),
                    subject,
                    types,
                    replacements,
                )
            })
            .collect()
    }
}

// Witness slots identify the declaration, while proof queries use its instantiated obligation.
pub(crate) struct InstantiatedParameterBound {
    declared: TraitRef,
    obligation: TraitObligation,
}

impl InstantiatedParameterBound {
    pub(crate) fn instantiate(
        declared: TraitRef,
        subject: TypeId,
        types: &mut CanonicalTypeStore,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<Self, TypeStoreError> {
        let arguments = types.substitution(declared.substitution())?.to_vec();
        let arguments = arguments
            .into_iter()
            .map(|argument| types.substitute(argument, replacements))
            .collect::<Result<Vec<_>, _>>()?;
        let substitution = types.intern_substitution(arguments)?;
        let trait_ref = TraitRef::new(declared.declaration().clone(), substitution);
        Ok(Self {
            declared,
            obligation: TraitObligation::new(subject, trait_ref),
        })
    }

    pub(crate) const fn declared(&self) -> &TraitRef {
        &self.declared
    }

    pub(crate) fn into_obligation(self) -> TraitObligation {
        self.obligation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalTarget, ExternalDefId, IntrinsicNominal, NominalHead, PrimitiveType};

    fn parameter(index: u32) -> GenericParameterId {
        GenericParameterId::Trusted {
            owner: ExternalDefId::new("example::pick").unwrap(),
            index,
        }
    }

    fn target(name: &str) -> CanonicalTarget {
        CanonicalTarget::External(ExternalDefId::new(name).unwrap())
    }

    #[test]
    fn instantiation_preserves_declared_identity_and_substitutes_later_parameters() {
        let mut types = CanonicalTypeStore::new().unwrap();
        let second = types.generic_parameter(parameter(1)).unwrap();
        let nested = types
            .nominal(NominalHead::Intrinsic(IntrinsicNominal::Opt), [second])
            .unwrap();
        let arguments = types.intern_substitution([nested, second]).unwrap();
        let alpha = TraitRef::new(target("example::Alpha"), arguments);
        let zulu = TraitRef::new(target("example::Zulu"), arguments);
        let signature = GenericParameterSignature::new(
            parameter(0),
            "A".into(),
            vec![zulu.clone(), alpha.clone()],
        );
        assert_eq!(signature.bounds(), [alpha.clone(), zulu.clone()]);

        let boolean = types.primitive(PrimitiveType::Bool).unwrap();
        let integer = types.primitive(PrimitiveType::I64).unwrap();
        let replacements = BTreeMap::from([(parameter(0), boolean), (parameter(1), integer)]);
        let instances = signature
            .instantiate_bounds(&mut types, &replacements)
            .unwrap();
        assert_eq!(instances.len(), 2);
        let opt_integer = types
            .nominal(NominalHead::Intrinsic(IntrinsicNominal::Opt), [integer])
            .unwrap();
        for (instance, declared) in instances.into_iter().zip([zulu, alpha]) {
            assert_eq!(instance.declared(), &declared);
            let obligation = instance.into_obligation();
            assert_eq!(obligation.subject(), boolean);
            assert_eq!(obligation.trait_ref().declaration(), declared.declaration());
            assert_eq!(
                types
                    .substitution(obligation.trait_ref().substitution())
                    .unwrap(),
                [opt_integer, integer]
            );
        }
        assert_eq!(types.substitution(arguments).unwrap(), [nested, second]);
    }

    #[test]
    fn an_unbounded_parameter_has_no_proof_requests() {
        let mut types = CanonicalTypeStore::new().unwrap();
        let integer = types.primitive(PrimitiveType::I64).unwrap();
        let signature = GenericParameterSignature::new(parameter(0), "A".into(), vec![]);
        assert!(
            signature
                .instantiate_bounds(&mut types, &BTreeMap::from([(parameter(0), integer)]))
                .unwrap()
                .is_empty()
        );
    }
}

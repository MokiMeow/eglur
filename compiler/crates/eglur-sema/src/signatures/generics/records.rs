use crate::{DuplicateFact, FactState, FactTable, GenericParameterId, SemanticFact};

use super::GenericParameterSignature;

/// A graph-local reference to an original generic-parameter definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenericParameterRecordId(usize);

impl GenericParameterRecordId {
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// Original definition identity kept outside semantic signature equality.
#[derive(Debug)]
pub struct GenericParameterRecord {
    id: GenericParameterRecordId,
    signature: GenericParameterSignature,
}

impl GenericParameterRecord {
    #[must_use]
    pub const fn id(&self) -> GenericParameterRecordId {
        self.id
    }

    #[must_use]
    pub const fn signature(&self) -> &GenericParameterSignature {
        &self.signature
    }
}

pub(crate) struct GenericParameterRecords {
    facts: FactTable<GenericParameterId, GenericParameterRecord>,
}

impl GenericParameterRecords {
    pub(crate) const fn new() -> Self {
        Self {
            facts: FactTable::new(),
        }
    }

    pub(crate) fn insert(
        &mut self,
        key: GenericParameterId,
        fact: SemanticFact<GenericParameterSignature>,
    ) -> Result<(), DuplicateFact> {
        let fact = match fact {
            SemanticFact::Known(signature) => SemanticFact::Known(GenericParameterRecord {
                id: GenericParameterRecordId(self.len()),
                signature,
            }),
            SemanticFact::Poisoned(poison) => SemanticFact::Poisoned(poison),
            SemanticFact::NotApplicable => SemanticFact::NotApplicable,
        };
        self.facts.insert(key, fact)
    }

    pub(crate) fn state(
        &self,
        key: &GenericParameterId,
    ) -> FactState<'_, GenericParameterSignature> {
        match self.record(key) {
            FactState::Known(record) => FactState::Known(record.signature()),
            FactState::Poisoned(poison) => FactState::Poisoned(poison),
            FactState::NotApplicable => FactState::NotApplicable,
            FactState::Unperformed => FactState::Unperformed,
        }
    }

    pub(crate) fn record(&self, key: &GenericParameterId) -> FactState<'_, GenericParameterRecord> {
        self.facts.state(key)
    }

    pub(crate) fn len(&self) -> usize {
        self.facts.iter().len()
    }

    pub(crate) fn has_poisoned(&self) -> bool {
        self.facts.has_poisoned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExternalDefId;

    fn parameter(index: u32) -> GenericParameterId {
        GenericParameterId::Trusted {
            owner: ExternalDefId::new("example::pick").unwrap(),
            index,
        }
    }

    fn signature(index: u32) -> GenericParameterSignature {
        GenericParameterSignature::new(parameter(index), "T".into(), Vec::new())
    }

    #[test]
    fn original_records_are_single_assignment_and_do_not_change_semantic_values() {
        let mut records = GenericParameterRecords::new();
        assert!(matches!(
            records.record(&parameter(0)),
            FactState::Unperformed
        ));
        records
            .insert(parameter(0), SemanticFact::Known(signature(0)))
            .unwrap();
        let FactState::Known(first) = records.record(&parameter(0)) else {
            panic!("known definition")
        };
        let first_id = first.id();
        assert_eq!(first.signature(), &signature(0));
        assert!(
            records
                .insert(parameter(0), SemanticFact::Known(signature(0)))
                .is_err()
        );
        assert_eq!(records.len(), 1);
        records
            .insert(parameter(1), SemanticFact::NotApplicable)
            .unwrap();
        records
            .insert(parameter(2), SemanticFact::Known(signature(2)))
            .unwrap();
        let FactState::Known(second) = records.record(&parameter(2)) else {
            panic!("known definition")
        };
        assert_ne!(first_id, second.id());
        assert!(matches!(
            records.state(&parameter(1)),
            FactState::NotApplicable
        ));
        let FactState::Known(retained) = records.record(&parameter(0)) else {
            panic!("retained definition")
        };
        assert_eq!(retained.id(), first_id);
        let FactState::Known(semantic) = records.state(&parameter(0)) else {
            panic!("semantic projection")
        };
        assert_eq!(semantic, &signature(0));
    }

    #[test]
    fn definition_insertion_order_does_not_change_signature_equality() {
        let mut left = GenericParameterRecords::new();
        let mut right = GenericParameterRecords::new();
        for index in [0, 1] {
            left.insert(parameter(index), SemanticFact::Known(signature(index)))
                .unwrap();
        }
        for index in [1, 0] {
            right
                .insert(parameter(index), SemanticFact::Known(signature(index)))
                .unwrap();
        }
        for index in [0, 1] {
            let FactState::Known(left) = left.record(&parameter(index)) else {
                panic!("left definition")
            };
            let FactState::Known(right) = right.record(&parameter(index)) else {
                panic!("right definition")
            };
            assert_ne!(left.id(), right.id());
            assert_eq!(left.signature(), right.signature());
        }
    }
}

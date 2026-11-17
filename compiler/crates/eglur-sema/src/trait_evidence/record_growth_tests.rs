use crate::record_reservation::observation::{assert_append, assert_growth_envelope, observe};
use crate::{CanonicalTypeStore, ModuleId, PrimitiveType};

use super::*;

#[test]
fn environment_records_reuse_dense_ids_and_preserve_contents_through_freeze() {
    let module = ModuleId::from_index(0).unwrap();
    let owner = DefId::from_index(module, 0).unwrap();
    let trait_target = CanonicalTarget::Source(DefId::from_index(module, 1).unwrap());
    let mut types = CanonicalTypeStore::new().unwrap();
    let subject = types.primitive(PrimitiveType::I32).unwrap();
    let substitution = types.intern_substitution([]).unwrap();
    let mut environments = Vec::new();
    let mut ids = BTreeMap::new();
    let mut reservations = Vec::new();
    for index in 0..1025 {
        let entries: Box<[EvidenceEntry]> = if index == 0 {
            Box::new([])
        } else {
            Box::new([EvidenceEntry {
                parameter: GenericParameterId::Source(owner),
                bound_index: index - 1,
                subject,
                trait_ref: TraitRef::new(trait_target.clone(), substitution),
                proof: EvidenceProofId::from_index(index - 1).unwrap(),
                destination: Some(WitnessSlotId::from_index(owner, index - 1).unwrap()),
            }])
        };
        let (id, observed) =
            observe(|| intern_environment(entries.clone(), &mut environments, &mut ids));
        let id = id.unwrap();
        assert_eq!(id.index(), index);
        assert_eq!(observed.len(), 1);
        assert_append(&observed[0], environments.len(), environments.capacity());
        assert_eq!(environments[id.index()].id(), id);
        assert_eq!(environments[id.index()].entries(), &*entries);
        assert_eq!(ids.get(&entries), Some(&id));
        reservations.extend(observed);
        assert_growth_envelope(
            &reservations,
            environments.len() * size_of::<EvidenceEnvironment>(),
        );

        let (hit, observed) = observe(|| intern_environment(entries, &mut environments, &mut ids));
        assert_eq!(hit.unwrap(), id);
        assert!(observed.is_empty());
        assert_eq!(environments.len(), index + 1);
    }
    let expected = environments.clone();
    assert_eq!(&*environments.into_boxed_slice(), expected);
}

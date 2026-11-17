use crate::record_reservation::observation::{
    ReservationObservation, assert_append, assert_growth_envelope, observe,
};

use super::*;

fn assert_type_append(store: &CanonicalTypeStore, observed: &[ReservationObservation]) {
    assert_eq!(observed.len(), 3);
    assert_append(&observed[0], store.types.len(), store.types.capacity());
    assert_append(
        &observed[1],
        store.may_need_generic_substitution.len(),
        store.may_need_generic_substitution.capacity(),
    );
    assert_append(
        &observed[2],
        store.ownership_presence.len(),
        store.ownership_presence.capacity(),
    );
    assert_eq!(store.types.len(), store.may_need_generic_substitution.len());
    assert_eq!(store.types.len(), store.ownership_presence.len());
}

fn assert_substitution_append(store: &CanonicalTypeStore, observed: &[ReservationObservation]) {
    assert_eq!(observed.len(), 2);
    assert_append(
        &observed[0],
        store.substitutions.len(),
        store.substitutions.capacity(),
    );
    assert_append(
        &observed[1],
        store.substitution_ownership_presence.len(),
        store.substitution_ownership_presence.capacity(),
    );
    assert_eq!(
        store.substitutions.len(),
        store.substitution_ownership_presence.len()
    );
}

#[test]
fn bootstrap_and_many_type_records_preserve_ids_metadata_and_growth_bound() {
    let (store, bootstrap) = observe(CanonicalTypeStore::new);
    let mut store = store.unwrap();
    assert_eq!(bootstrap.len(), 3 + 2);
    assert_type_append(&store, &bootstrap[..3]);
    assert_substitution_append(&store, &bootstrap[3..]);
    let mut reservations = bootstrap[..3].to_vec();
    let mut root = store.never();
    for _ in 0..1025 {
        let previous = root;
        let count = store.type_count();
        let (result, observed) = observe(|| store.reference(ReferenceMutability::Shared, previous));
        root = result.unwrap();
        assert_eq!(root.index(), count);
        assert_type_append(&store, &observed);
        assert_eq!(
            store.kind(root).unwrap(),
            &TypeKind::Reference {
                referent: previous,
                mutability: ReferenceMutability::Shared,
            }
        );
        assert!(!store.contains_ownership(root).unwrap());
        assert!(!store.may_need_generic_substitution[root.index()]);
        reservations.extend(observed);
        let headers = size_of::<TypeKind>() + size_of::<bool>() + size_of::<Option<bool>>();
        assert_growth_envelope(&reservations, store.type_count() * headers);

        let (hit, observed) = observe(|| store.reference(ReferenceMutability::Shared, previous));
        assert_eq!(hit.unwrap(), root);
        assert!(observed.is_empty());
        assert_eq!(store.type_count(), count + 1);
    }

    let count = store.type_count();
    let invalid = TypeId::from_index(count + 1).unwrap();
    let (error, observed) = observe(|| store.reference(ReferenceMutability::Shared, invalid));
    assert_eq!(error, Err(TypeStoreError::UnknownType(invalid)));
    assert!(observed.is_empty());
    assert_eq!(store.type_count(), count);
    assert_eq!(store.type_ids.len(), count);
    assert_eq!(store.may_need_generic_substitution.len(), count);
    assert_eq!(store.ownership_presence.len(), count);
}

#[test]
fn substitution_growth_preserves_order_ownership_and_invalid_input_prefix() {
    let (store, bootstrap) = observe(CanonicalTypeStore::new);
    let mut store = store.unwrap();
    assert_eq!(bootstrap.len(), 3 + 2);
    let mut reservations = bootstrap[3..].to_vec();
    let integer = store.primitive(PrimitiveType::I32).unwrap();
    let (_, observed) = observe(|| store.intern_substitution([integer]));
    assert_substitution_append(&store, &observed);
    reservations.extend(observed);
    let owned = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [integer])
        .unwrap();
    for length in 0..1025 {
        let arguments = vec![owned; length];
        let before = store.substitution_count();
        let (result, observed) = observe(|| store.intern_substitution(arguments.iter().copied()));
        let id = result.unwrap();
        assert_eq!(store.substitution(id).unwrap(), arguments);
        assert_eq!(
            store.substitution_ownership_presence[id.index()],
            Some(length > 0)
        );
        if length == 0 {
            assert!(observed.is_empty());
        } else {
            assert_eq!(id.index(), before);
            assert_substitution_append(&store, &observed);
            reservations.extend(observed);
        }
        let (hit, observed) = observe(|| store.intern_substitution(arguments));
        assert_eq!(hit.unwrap(), id);
        assert!(observed.is_empty());
    }
    let count = store.substitution_count();
    let capacities = (
        store.substitutions.capacity(),
        store.substitution_ownership_presence.capacity(),
    );
    let invalid = TypeId::from_index(store.type_count() + 1).unwrap();
    let (error, observed) = observe(|| store.intern_substitution([integer, invalid]));
    assert_eq!(error, Err(TypeStoreError::UnknownType(invalid)));
    assert!(observed.is_empty());
    assert_eq!(store.substitution_count(), count);
    assert_eq!(
        capacities,
        (
            store.substitutions.capacity(),
            store.substitution_ownership_presence.capacity()
        )
    );
    let headers = size_of::<Box<[TypeId]>>() + size_of::<Option<bool>>();
    assert_growth_envelope(&reservations, count * headers);
}

#[test]
fn parallel_arrays_with_different_capacities_are_prepared_independently() {
    let mut store = CanonicalTypeStore::new().unwrap();
    store.may_need_generic_substitution.reserve_exact(91);
    store.ownership_presence.reserve_exact(17);
    let (integer, observed) = observe(|| store.primitive(PrimitiveType::I32));
    let integer = integer.unwrap();
    assert_type_append(&store, &observed);
    assert_eq!(observed[1].requested_capacity, None);
    assert_eq!(observed[2].requested_capacity, None);

    store.substitution_ownership_presence.reserve_exact(43);
    let (id, observed) = observe(|| store.intern_substitution([integer, integer]));
    let id = id.unwrap();
    assert_substitution_append(&store, &observed);
    assert_eq!(observed[1].requested_capacity, None);
    assert_eq!(store.substitution(id).unwrap(), [integer, integer]);
}

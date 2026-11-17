use std::cell::Cell;

use super::*;

#[test]
fn sized_input_forms_reuse_the_same_ordered_substitution() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let integer = store.primitive(PrimitiveType::I32).unwrap();
    let boolean = store.primitive(PrimitiveType::Bool).unwrap();
    let arguments = [integer, boolean];
    let expected = store.intern_substitution(arguments).unwrap();
    let count = store.substitution_count();

    let borrowed = store
        .intern_substitution(arguments.iter().copied())
        .unwrap();
    let owned = store.intern_substitution(arguments.to_vec()).unwrap();
    let boxed = store
        .intern_substitution(arguments.to_vec().into_boxed_slice())
        .unwrap();
    let taken = store
        .intern_substitution([integer, boolean, integer].into_iter().take(2))
        .unwrap();

    assert_eq!([borrowed, owned, boxed, taken], [expected; 4]);
    assert_eq!(store.substitution(expected).unwrap(), arguments);
    assert_eq!(store.substitution_count(), count);
}

#[test]
fn empty_sized_inputs_reuse_the_bootstrap_substitution() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let count = store.substitution_count();
    let expected = store.intern_substitution([]).unwrap();

    assert_eq!(
        store.intern_substitution(Vec::<TypeId>::new()).unwrap(),
        expected
    );
    assert_eq!(
        store.intern_substitution(std::iter::empty()).unwrap(),
        expected
    );
    assert!(store.substitution(expected).unwrap().is_empty());
    assert_eq!(store.substitution_count(), count);
}

#[test]
fn sized_repetition_preserves_arity_multiplicity_and_order() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let integer = store.primitive(PrimitiveType::I32).unwrap();
    let boolean = store.primitive(PrimitiveType::Bool).unwrap();
    let repeated = store
        .intern_substitution(std::iter::repeat_n(integer, 257))
        .unwrap();
    let values = store.substitution(repeated).unwrap();

    assert_eq!(values.len(), 257);
    assert!(values.iter().all(|&value| value == integer));
    assert_eq!(
        store.intern_substitution(vec![integer; 257]).unwrap(),
        repeated
    );

    let forward = store
        .intern_substitution([integer, integer, boolean])
        .unwrap();
    let reverse = store
        .intern_substitution([integer, integer, boolean].into_iter().rev())
        .unwrap();
    assert_ne!(forward, reverse);
    assert_eq!(
        store.substitution(reverse).unwrap(),
        [boolean, integer, integer]
    );
}

#[test]
fn sized_mapping_consumes_each_argument_once_even_on_an_intern_hit() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let integer = store.primitive(PrimitiveType::I32).unwrap();
    let boolean = store.primitive(PrimitiveType::Bool).unwrap();
    let arguments = [integer, boolean];
    let expected = store.intern_substitution(arguments).unwrap();
    let input = [boolean, integer];
    let visits = Cell::new(0);
    let count = store.substitution_count();

    let actual = store
        .intern_substitution(input.into_iter().map(|argument| {
            visits.set(visits.get() + 1);
            if argument == boolean {
                integer
            } else {
                boolean
            }
        }))
        .unwrap();

    assert_eq!(actual, expected);
    assert_eq!(visits.get(), input.len());
    assert_eq!(store.substitution_count(), count);
}

#[test]
fn invalid_sized_input_does_not_publish_partial_records() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let integer = store.primitive(PrimitiveType::I32).unwrap();
    let invalid = TypeId::from_index(store.type_count() + 7).unwrap();
    let count = store.substitution_count();
    let types = store.type_count();

    assert_eq!(
        store.intern_substitution([integer, invalid, integer]),
        Err(TypeStoreError::UnknownType(invalid))
    );
    assert_eq!(store.substitution_count(), count);
    assert_eq!(store.type_count(), types);
    assert_eq!(store.substitution_ids.len(), count);
    assert_eq!(store.substitution_ownership_presence.len(), count);

    let valid = store.intern_substitution([integer, integer]).unwrap();
    assert_eq!(valid.index(), count);
    assert_eq!(store.substitution(valid).unwrap(), [integer, integer]);
}

#[test]
fn sized_input_forms_preserve_ownership_metadata() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let integer = store.primitive(PrimitiveType::I32).unwrap();
    let owned = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [integer])
        .unwrap();
    let plain = store.intern_substitution([integer]).unwrap();
    let containing = store.intern_substitution(vec![owned, integer]).unwrap();

    assert_eq!(
        store.substitution_ownership_presence[plain.index()],
        Some(false)
    );
    assert_eq!(
        store.substitution_ownership_presence[containing.index()],
        Some(true)
    );
    assert_eq!(
        store
            .intern_substitution([owned, integer].iter().copied())
            .unwrap(),
        containing
    );
}

use std::cell::Cell;

use super::tests::SUBSTITUTION_READS;
use super::*;

thread_local! {
    static FACT_WORK: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

pub(super) fn observe_admission() {
    let (nodes, edges) = FACT_WORK.get();
    FACT_WORK.set((nodes + 1, edges));
}

pub(super) fn observe_child_fact() {
    let (nodes, edges) = FACT_WORK.get();
    FACT_WORK.set((nodes, edges + 1));
}

fn parameter(owner: &str) -> GenericParameterId {
    GenericParameterId::Trusted {
        owner: ExternalDefId::new(owner).unwrap(),
        index: 0,
    }
}

fn target(key: &str) -> CanonicalTarget {
    CanonicalTarget::External(ExternalDefId::new(key).unwrap())
}

#[test]
fn closed_kinds_skip_payload_reads_without_skipping_validation() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let integer = store.primitive(PrimitiveType::I64).unwrap();
    let text = store.primitive(PrimitiveType::Str).unwrap();
    let self_type = store.self_type(target("std::Node")).unwrap();
    let owned = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [integer])
        .unwrap();
    let reference = store.reference(ReferenceMutability::Shared, owned).unwrap();
    let pointer = store
        .raw_pointer(RawPointerMutability::Mutable, integer)
        .unwrap();
    let tuple = store.tuple([owned, pointer, reference]).unwrap();
    let callable = store.callable([tuple, integer], self_type).unwrap();
    let union = store.error_union([text, integer, callable]).unwrap();
    let arguments = store.intern_substitution([pointer, reference]).unwrap();
    let projection = store
        .associated_projection(AssociatedProjection::new(
            integer,
            TraitRef::new(target("std::Linked"), arguments),
            target("std::Linked::Item"),
        ))
        .unwrap();
    let roots = [
        store.never(),
        integer,
        text,
        self_type,
        owned,
        reference,
        pointer,
        tuple,
        callable,
        union,
        projection,
    ];
    let replacement = BTreeMap::from([(parameter("std::Unused"), text)]);
    let count = store.type_count();
    let substitutions = store.substitution_count();
    let facts = FACT_WORK.get();
    SUBSTITUTION_READS.set(0);

    for root in roots {
        assert_eq!(store.substitute(root, &replacement), Ok(root));
    }
    assert_eq!(SUBSTITUTION_READS.get(), 0);
    assert_eq!(FACT_WORK.get(), facts);
    assert_eq!(store.type_count(), count);
    assert_eq!(store.substitution_count(), substitutions);

    let unknown_root = TypeId::from_index(count + 10).unwrap();
    let unknown_value = TypeId::from_index(count + 11).unwrap();
    let bad = BTreeMap::from([(parameter("std::Unused"), unknown_value)]);
    assert_eq!(
        store.substitute(tuple, &bad),
        Err(TypeStoreError::UnknownType(unknown_value))
    );
    assert_eq!(
        store.substitute(unknown_root, &bad),
        Err(TypeStoreError::UnknownType(unknown_root))
    );
    assert_eq!(
        store.reference(ReferenceMutability::Shared, unknown_root),
        Err(TypeStoreError::UnknownType(unknown_root))
    );
    assert_eq!(store.may_need_generic_substitution.len(), count);
    assert_eq!(FACT_WORK.get(), facts);
    assert_eq!(SUBSTITUTION_READS.get(), 0);
}

#[test]
fn closed_projection_bases_do_not_hide_parameters_in_trait_arguments() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let integer = store.primitive(PrimitiveType::I64).unwrap();
    let text = store.primitive(PrimitiveType::Str).unwrap();
    let p = parameter("std::P");
    let p_type = store.generic_parameter(p.clone()).unwrap();
    let wrapper = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [p_type])
        .unwrap();
    let arguments = store.intern_substitution([wrapper]).unwrap();
    let projection = store
        .associated_projection(AssociatedProjection::new(
            integer,
            TraitRef::new(target("std::Linked"), arguments),
            target("std::Linked::Item"),
        ))
        .unwrap();

    let actual = store
        .substitute(projection, &BTreeMap::from([(p, text)]))
        .unwrap();
    let expected_wrapper = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [text])
        .unwrap();
    let expected_arguments = store.intern_substitution([expected_wrapper]).unwrap();
    let expected = store
        .associated_projection(AssociatedProjection::new(
            integer,
            TraitRef::new(target("std::Linked"), expected_arguments),
            target("std::Linked::Item"),
        ))
        .unwrap();
    assert_eq!(actual, expected);
    assert_ne!(actual, projection);
}

#[test]
fn closed_suffixes_and_wide_roots_only_read_admission_operands_once() {
    for size in [8, 32, 128, 20_000] {
        FACT_WORK.set((0, 0));
        let mut store = CanonicalTypeStore::new().unwrap();
        let scalar = store.primitive(PrimitiveType::I64).unwrap();
        let mut root = scalar;
        let mut roots = Vec::new();
        for _ in 0..size {
            let previous = root;
            root = store.tuple([previous, scalar]).unwrap();
            let facts = FACT_WORK.get();
            assert_eq!(store.tuple([previous, scalar]).unwrap(), root);
            assert_eq!(FACT_WORK.get(), facts);
            roots.push(root);
        }
        roots.push(store.tuple(std::iter::repeat_n(root, 1024)).unwrap());
        assert_eq!(FACT_WORK.get(), (size + 3, 2 * size + 1024));
        assert_eq!(
            store.may_need_generic_substitution.len(),
            store.type_count()
        );
        let map = BTreeMap::from([(parameter("std::Unused"), scalar)]);
        SUBSTITUTION_READS.set(0);

        for root in roots {
            assert_eq!(store.substitute(root, &map), Ok(root));
        }
        assert_eq!(FACT_WORK.get(), (size + 3, 2 * size + 1024));
        assert_eq!(SUBSTITUTION_READS.get(), 0);
    }
}

#[test]
fn appended_graphs_and_repeated_queries_preserve_existing_facts() {
    FACT_WORK.set((0, 0));
    let mut store = CanonicalTypeStore::new().unwrap();
    let scalar = store.primitive(PrimitiveType::I64).unwrap();
    let mut root = scalar;
    let map = BTreeMap::from([(parameter("std::Unused"), scalar)]);
    SUBSTITUTION_READS.set(0);

    for _ in 0..96 {
        root = store.tuple([root, scalar]).unwrap();
        assert_eq!(store.substitute(root, &map), Ok(root));
    }
    let facts = FACT_WORK.get();
    assert_eq!(facts, (98, 192));
    for _ in 0..1024 {
        assert_eq!(store.substitute(root, &map), Ok(root));
        assert_eq!(store.substitute(scalar, &map), Ok(scalar));
    }
    assert_eq!(FACT_WORK.get(), facts);
    assert_eq!(SUBSTITUTION_READS.get(), 0);
}

#[test]
fn parameter_facts_are_store_local_and_replacement_independent() {
    let mut left = CanonicalTypeStore::new().unwrap();
    let mut right = CanonicalTypeStore::new().unwrap();
    let left_scalar = left.primitive(PrimitiveType::I64).unwrap();
    let right_scalar = right.primitive(PrimitiveType::I64).unwrap();
    let p = parameter("std::P");
    let left_child = left.generic_parameter(p.clone()).unwrap();
    let right_child = right.primitive(PrimitiveType::Str).unwrap();
    let left_root = left.tuple([left_child, left_scalar]).unwrap();
    let right_root = right.tuple([right_child, right_scalar]).unwrap();
    assert_eq!(left_root, right_root);

    for replacement in [left_scalar, left_child, left_scalar] {
        let map = BTreeMap::from([(p.clone(), replacement)]);
        let expected = left.tuple([replacement, left_scalar]).unwrap();
        assert_eq!(left.substitute(left_root, &map), Ok(expected));
        assert_eq!(right.substitute(right_root, &map), Ok(right_root));
    }
}

#[test]
fn unknown_private_metadata_keeps_the_fallible_substitution_path() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let scalar = store.primitive(PrimitiveType::I64).unwrap();
    let unknown = SubstId::from_index(store.substitution_count() + 10).unwrap();
    let nominal = store
        .intern_type(TypeKind::Nominal {
            head: NominalHead::Intrinsic(IntrinsicNominal::Opt),
            substitution: unknown,
        })
        .unwrap();
    let projection = store
        .intern_type(TypeKind::AssociatedProjection(AssociatedProjection::new(
            scalar,
            TraitRef::new(target("std::Linked"), unknown),
            target("std::Linked::Item"),
        )))
        .unwrap();
    let map = BTreeMap::from([(parameter("std::Unused"), scalar)]);
    for root in [nominal, projection] {
        assert_eq!(
            store.substitute(root, &map),
            Err(TypeStoreError::UnknownSubstitution(unknown))
        );
    }
}

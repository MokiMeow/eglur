use std::cell::Cell;

use super::*;

thread_local! {
    static KIND_READS: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(super) fn observe_kind_read() {
    KIND_READS.with(|reads| {
        if let Some(count) = reads.get() {
            reads.set(Some(count + 1));
        }
    });
}

fn traversed_ownership(store: &CanonicalTypeStore, root: TypeId) -> bool {
    store
        .contains(root, |kind| {
            matches!(
                kind,
                TypeKind::Nominal {
                    head: NominalHead::Intrinsic(IntrinsicNominal::Own),
                    ..
                }
            )
        })
        .unwrap()
}

#[test]
fn admitted_presence_matches_the_payload_graph_without_expanding_projections() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let scalar = store.primitive(PrimitiveType::I64).unwrap();
    let root = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [scalar])
        .unwrap();
    let mut seeds = vec![store.never(), scalar, root];
    let trait_target = CanonicalTarget::External(ExternalDefId::new("fixture::Trait").unwrap());
    let associated = CanonicalTarget::External(ExternalDefId::new("fixture::Item").unwrap());
    seeds.push(store.self_type(trait_target.clone()).unwrap());
    seeds.push(
        store
            .generic_parameter(GenericParameterId::Trusted {
                owner: ExternalDefId::new("fixture::generic").unwrap(),
                index: 0,
            })
            .unwrap(),
    );
    let empty = store.intern_substitution([]).unwrap();
    for child in [scalar, root] {
        seeds.push(store.reference(ReferenceMutability::Shared, child).unwrap());
        seeds.push(
            store
                .raw_pointer(RawPointerMutability::Const, child)
                .unwrap(),
        );
        seeds.push(store.tuple([child, scalar]).unwrap());
        seeds.push(store.callable([child], scalar).unwrap());
        seeds.push(store.callable([scalar], child).unwrap());
        seeds.push(store.error_union([child, scalar]).unwrap());
        for head in [
            IntrinsicNominal::Own,
            IntrinsicNominal::Box,
            IntrinsicNominal::Opt,
        ] {
            seeds.push(
                store
                    .nominal(NominalHead::Intrinsic(head), [child])
                    .unwrap(),
            );
        }
        seeds.push(
            store
                .nominal(
                    NominalHead::Intrinsic(IntrinsicNominal::Res),
                    [child, scalar],
                )
                .unwrap(),
        );
        seeds.push(
            store
                .associated_projection(AssociatedProjection::new(
                    child,
                    TraitRef::new(trait_target.clone(), empty),
                    associated.clone(),
                ))
                .unwrap(),
        );
    }
    let shared = store.intern_substitution([scalar, root]).unwrap();
    for name in ["fixture::A", "fixture::B"] {
        let head =
            NominalHead::Declaration(CanonicalTarget::External(ExternalDefId::new(name).unwrap()));
        store.register_nominal_head(head.clone(), 2).unwrap();
        seeds.push(
            store
                .intern_type(TypeKind::Nominal {
                    head,
                    substitution: shared,
                })
                .unwrap(),
        );
    }
    for round in 0..5 {
        let children = seeds.clone();
        for (index, &child) in children.iter().enumerate() {
            let neighbor = children[(index + round + 1) % children.len()];
            seeds.push(store.tuple([child, neighbor]).unwrap());
            seeds.push(store.callable([child], neighbor).unwrap());
        }
        for &ty in &seeds {
            assert_eq!(
                store.contains_ownership(ty).unwrap(),
                traversed_ownership(&store, ty)
            );
        }
    }
}

#[test]
fn rewritten_generic_payloads_admit_their_own_presence_facts() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let parameter = GenericParameterId::Trusted {
        owner: ExternalDefId::new("fixture::generic").unwrap(),
        index: 0,
    };
    let generic = store.generic_parameter(parameter.clone()).unwrap();
    let scalar = store.primitive(PrimitiveType::I64).unwrap();
    let owned = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [scalar])
        .unwrap();
    let template = store.callable([generic], scalar).unwrap();
    assert!(!store.contains_ownership(template).unwrap());
    for replacement in [scalar, owned] {
        let concrete = store
            .substitute(
                template,
                &BTreeMap::from([(parameter.clone(), replacement)]),
            )
            .unwrap();
        assert_eq!(
            store.contains_ownership(concrete).unwrap(),
            replacement == owned
        );
        assert_eq!(
            store.contains_ownership(concrete).unwrap(),
            traversed_ownership(&store, concrete)
        );
    }
    assert!(!store.contains_ownership(template).unwrap());
}

#[test]
fn repeated_presence_queries_read_only_the_root_of_deep_shared_graphs() {
    for owned in [false, true] {
        let mut store = CanonicalTypeStore::new().unwrap();
        let scalar = store.primitive(PrimitiveType::Bool).unwrap();
        let mut root = if owned {
            store
                .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [scalar])
                .unwrap()
        } else {
            scalar
        };
        for _ in 0..4096 {
            root = store.tuple([root, root]).unwrap();
        }
        assert_eq!(traversed_ownership(&store, root), owned);
        let queries = 256;
        KIND_READS.with(|reads| reads.set(Some(0)));
        for _ in 0..queries {
            assert_eq!(store.contains_ownership(root).unwrap(), owned);
        }
        let reads = KIND_READS.with(|reads| reads.replace(None).unwrap());
        assert!(
            reads <= queries * 2,
            "{reads} kind reads for {queries} queries"
        );
    }
}

#[test]
fn invalid_ids_never_read_an_unadmitted_ownership_fact() {
    let store = CanonicalTypeStore::new().unwrap();
    let invalid = TypeId::from_index(store.types.len()).unwrap();
    assert_eq!(
        store.contains_ownership(invalid),
        Err(TypeStoreError::UnknownType(invalid))
    );
}

#[test]
fn incomplete_private_metadata_keeps_first_errors_and_positive_shortcuts() {
    let mut store = CanonicalTypeStore::new().unwrap();
    let scalar = store.primitive(PrimitiveType::I64).unwrap();
    let owned = store
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [scalar])
        .unwrap();
    let unknown = SubstId::from_index(store.substitution_count() + 100).unwrap();
    let malformed = store
        .intern_type(TypeKind::Nominal {
            head: NominalHead::Intrinsic(IntrinsicNominal::Opt),
            substitution: unknown,
        })
        .unwrap();
    assert_eq!(store.ownership_presence[malformed.index()], None);
    assert_eq!(
        store.contains_ownership(malformed),
        Err(TypeStoreError::UnknownSubstitution(unknown))
    );

    for (children, expected) in [
        ([malformed, owned], Ok(true)),
        (
            [owned, malformed],
            Err(TypeStoreError::UnknownSubstitution(unknown)),
        ),
    ] {
        let tuple = store.tuple(children).unwrap();
        assert_eq!(store.ownership_presence[tuple.index()], None);
        assert_eq!(store.contains_ownership(tuple), expected);
        let nominal = store
            .nominal(NominalHead::Intrinsic(IntrinsicNominal::Res), children)
            .unwrap();
        assert_eq!(store.ownership_presence[nominal.index()], None);
        assert_eq!(store.contains_ownership(nominal), expected);
    }
}

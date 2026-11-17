use super::*;
use crate::{
    AssociatedProjection, ExternalDefId, GenericParameterId, PrimitiveType, RawPointerMutability,
    ReferenceMutability, TypeStoreError,
};

fn parameter(owner: &str, index: u32) -> GenericParameterId {
    GenericParameterId::Trusted {
        owner: ExternalDefId::new(owner).unwrap(),
        index,
    }
}

fn target(key: &str) -> CanonicalTarget {
    CanonicalTarget::External(ExternalDefId::new(key).unwrap())
}

fn opt(types: &mut CanonicalTypeStore, inner: TypeId) -> TypeId {
    types
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Opt), [inner])
        .unwrap()
}

fn original_match(
    types: &CanonicalTypeStore,
    root: TypeId,
    slot: &GenericParameterId,
    visited: &mut BTreeSet<TypeId>,
    substitutions: &mut BTreeSet<TypeId>,
) -> Result<Option<bool>, TypeStoreError> {
    let mut current = root;
    let mut grows = false;
    loop {
        visited.insert(current);
        match types.kind(current)? {
            TypeKind::GenericParameter(parameter) if parameter == slot => return Ok(Some(grows)),
            TypeKind::Nominal {
                head: NominalHead::Intrinsic(IntrinsicNominal::Opt),
                substitution,
            } => {
                substitutions.insert(current);
                let [inner] = types.substitution(*substitution)? else {
                    return Ok(None);
                };
                current = *inner;
                grows = true;
            }
            _ => return Ok(None),
        }
    }
}

#[test]
fn completed_tails_preserve_slot_owner_growth_and_terminal_kind() {
    let mut types = CanonicalTypeStore::new().unwrap();
    let parameters = [
        parameter("owner", 0),
        parameter("owner", 1),
        parameter("other", 0),
    ];
    let slots = parameters
        .iter()
        .map(|parameter| types.generic_parameter(parameter.clone()).unwrap())
        .collect::<Vec<_>>();
    let integer = types.primitive(PrimitiveType::I64).unwrap();
    let self_type = types.self_type(target("SelfOwner")).unwrap();
    let tuple = types.tuple([slots[0], integer]).unwrap();
    let callable = types.callable([slots[0]], slots[1]).unwrap();
    let reference = types
        .reference(ReferenceMutability::Shared, slots[0])
        .unwrap();
    let pointer = types
        .raw_pointer(RawPointerMutability::Mutable, slots[0])
        .unwrap();
    let boxed = types
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Box), [slots[0]])
        .unwrap();
    let owned = types
        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [slots[0]])
        .unwrap();
    let result = types
        .nominal(
            NominalHead::Intrinsic(IntrinsicNominal::Res),
            [slots[0], integer],
        )
        .unwrap();
    let union = types.error_union([slots[0], integer]).unwrap();
    let substitution = types.intern_substitution([slots[1]]).unwrap();
    let projection = types
        .associated_projection(AssociatedProjection::new(
            slots[0],
            TraitRef::new(target("Trait"), substitution),
            target("Trait::Item"),
        ))
        .unwrap();
    let terminals = [
        slots[0],
        slots[1],
        slots[2],
        integer,
        types.never(),
        self_type,
        tuple,
        callable,
        reference,
        pointer,
        boxed,
        owned,
        result,
        union,
        projection,
    ];
    let mut roots = Vec::new();
    for terminal in terminals {
        let mut root = terminal;
        roots.push(root);
        for _ in 0..6 {
            root = opt(&mut types, root);
            roots.push(root);
        }
    }

    for reverse in [false, true] {
        let mut chains = WrapperChains::new(&types);
        let mut visited = BTreeSet::new();
        let mut substitutions = BTreeSet::new();
        for _ in 0..2 {
            for index in 0..roots.len() {
                let root = roots[if reverse {
                    roots.len() - index - 1
                } else {
                    index
                }];
                for (&slot, parameter) in slots.iter().zip(&parameters) {
                    assert_eq!(
                        chains.match_slot(root, slot),
                        original_match(&types, root, parameter, &mut visited, &mut substitutions),
                        "root={root:?}, slot={slot:?}"
                    );
                }
            }
        }
        assert_eq!(chains.kind_reads, visited.len());
        assert_eq!(chains.substitution_reads, substitutions.len());
        assert_eq!(chains.tails.len(), visited.len());
    }
}

#[test]
fn shared_positive_and_rejected_suffixes_are_inspected_once_without_recursion() {
    for depth in [8, 512, 20_000] {
        let mut types = CanonicalTypeStore::new().unwrap();
        let slot = types.generic_parameter(parameter("owner", 0)).unwrap();
        let integer = types.primitive(PrimitiveType::I64).unwrap();
        let mut positive = vec![slot];
        let mut negative = vec![integer];
        for _ in 0..depth {
            positive.push(opt(&mut types, *positive.last().unwrap()));
            negative.push(opt(&mut types, *negative.last().unwrap()));
        }
        let mut unused = integer;
        for _ in 0..depth {
            unused = types.tuple([unused, integer]).unwrap();
        }

        let mut chains = WrapperChains::new(&types);
        for _ in 0..2 {
            for (index, (&accepted, &rejected)) in positive.iter().zip(&negative).enumerate().rev()
            {
                assert_eq!(chains.match_slot(accepted, slot).unwrap(), Some(index != 0));
                assert_eq!(chains.match_slot(rejected, slot).unwrap(), None);
            }
        }
        assert_eq!(chains.kind_reads, positive.len() + negative.len());
        assert_eq!(
            chains.substitution_reads,
            positive.len() + negative.len() - 2
        );
        assert_eq!(chains.tails.len(), positive.len() + negative.len());
        assert!(!chains.tails.contains_key(&unused));
    }
}

#[test]
fn lookup_errors_leave_no_completed_tail_and_do_not_poison_other_queries() {
    let mut types = CanonicalTypeStore::new().unwrap();
    let slot = types.generic_parameter(parameter("owner", 0)).unwrap();
    let wrapped = opt(&mut types, slot);
    let unknown = TypeId::from_index(types.type_count()).unwrap();
    let mut chains = WrapperChains::new(&types);
    for _ in 0..2 {
        assert_eq!(
            chains.match_slot(unknown, slot),
            Err(TypeStoreError::UnknownType(unknown))
        );
        assert!(!chains.tails.contains_key(&unknown));
        assert_eq!(chains.match_slot(wrapped, slot).unwrap(), Some(true));
        assert_eq!(chains.match_slot(slot, slot).unwrap(), Some(false));
        assert_eq!(chains.tails.len(), 2);
    }
}

#[test]
fn completed_tail_identity_is_local_to_one_immutable_store() {
    let mut first = CanonicalTypeStore::new().unwrap();
    let mut second = CanonicalTypeStore::new().unwrap();
    let first_slot = first.generic_parameter(parameter("owner", 0)).unwrap();
    let second_leaf = second.primitive(PrimitiveType::I64).unwrap();
    assert_eq!(first_slot, second_leaf);
    let first_root = opt(&mut first, first_slot);
    let second_root = opt(&mut second, second_leaf);
    assert_eq!(first_root, second_root);
    let second_slot = second.generic_parameter(parameter("owner", 0)).unwrap();
    assert_eq!(
        WrapperChains::new(&first)
            .match_slot(first_root, first_slot)
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        WrapperChains::new(&second)
            .match_slot(second_root, second_slot)
            .unwrap(),
        None
    );
}

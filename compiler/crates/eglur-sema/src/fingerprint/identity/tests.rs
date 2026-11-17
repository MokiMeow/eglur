use super::*;
use crate::{AssociatedProjection, PrimitiveType};

fn external(name: &str) -> CanonicalTarget {
    CanonicalTarget::External(ExternalDefId::new(name).unwrap())
}

fn stable_external(target: &CanonicalTarget) -> StableTargetKey {
    let CanonicalTarget::External(target) = target else {
        panic!("fixture uses only external targets");
    };
    StableTargetKey::External(target.clone())
}

fn encode(value: &StableType) -> FingerprintDigest {
    let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Body);
    value.write_to(&mut builder);
    builder.finish()
}

#[test]
fn nested_stable_types_build_encode_compare_and_drop_on_the_default_stack() {
    enum Shape {
        Tuple,
        Reference,
        RawPointer,
        Callable,
        Nominal,
        Projection,
    }

    for shape in [
        Shape::Tuple,
        Shape::Reference,
        Shape::RawPointer,
        Shape::Callable,
        Shape::Nominal,
        Shape::Projection,
    ] {
        let mut types = CanonicalTypeStore::new().unwrap();
        let scalar = types.primitive(PrimitiveType::I64).unwrap();
        let arguments = types.intern_substitution([scalar]).unwrap();
        let mut ty = scalar;
        for _ in 0..240 {
            ty = match shape {
                Shape::Tuple => types.tuple([scalar, ty]),
                Shape::Reference => types.reference(ReferenceMutability::Mutable, ty),
                Shape::RawPointer => types.raw_pointer(RawPointerMutability::Const, ty),
                Shape::Callable => types.callable([scalar], ty),
                Shape::Nominal => {
                    types.nominal(NominalHead::Intrinsic(IntrinsicNominal::Opt), [ty])
                }
                Shape::Projection => types.associated_projection(AssociatedProjection::new(
                    ty,
                    TraitRef::new(external("probe::Trait"), arguments),
                    external("probe::Trait::Item"),
                )),
            }
            .unwrap();
        }

        let stable = StableType::from_type_with(ty, &BTreeMap::new(), &types, &mut |target| {
            Ok(stable_external(target))
        })
        .unwrap();
        let copy = stable.clone();
        assert_eq!(stable.cmp(&copy), std::cmp::Ordering::Equal);
        assert_eq!(encode(&stable), encode(&copy));
        drop(copy);
        drop(stable);
    }
}

#[test]
fn mixed_type_children_preserve_shape_and_callback_order() {
    let mut types = CanonicalTypeStore::new().unwrap();
    let first = types.self_type(external("probe::First")).unwrap();
    let last = types.self_type(external("probe::Last")).unwrap();
    let shared = types.reference(ReferenceMutability::Shared, first).unwrap();
    let pointer = types
        .raw_pointer(RawPointerMutability::Mutable, last)
        .unwrap();
    let head = NominalHead::Declaration(external("probe::Pair"));
    types.register_nominal_head(head.clone(), 2).unwrap();
    let pair = types.nominal(head, [shared, pointer]).unwrap();
    let nullary = types.callable([], types.never()).unwrap();
    let result = types.tuple([first, nullary, last]).unwrap();
    let callable = types.callable([pair, nullary], result).unwrap();

    let mut observed = Vec::new();
    let stable = StableType::from_type_with(callable, &BTreeMap::new(), &types, &mut |target| {
        observed.push(target.clone());
        Ok(stable_external(target))
    })
    .unwrap();
    let first = StableType::SelfType(stable_external(&external("probe::First")));
    let last = StableType::SelfType(stable_external(&external("probe::Last")));
    let nullary = StableType::Callable {
        parameters: Box::new([]),
        result: Box::new(StableType::Never),
    };
    let expected = StableType::Callable {
        parameters: Box::new([
            StableType::Nominal {
                head: StableNominalHead::Declaration(stable_external(&external("probe::Pair"))),
                arguments: Box::new([
                    StableType::Reference {
                        mutability: ReferenceMutability::Shared,
                        referent: Box::new(first.clone()),
                    },
                    StableType::RawPointer {
                        mutability: RawPointerMutability::Mutable,
                        referent: Box::new(last.clone()),
                    },
                ]),
            },
            nullary.clone(),
        ]),
        result: Box::new(StableType::Tuple(Box::new([first, nullary, last]))),
    };
    assert_eq!(stable, expected);
    assert_eq!(encode(&stable), encode(&expected));
    assert_eq!(
        observed,
        [
            "probe::Pair",
            "probe::First",
            "probe::Last",
            "probe::First",
            "probe::Last"
        ]
        .map(external)
    );
}

#[test]
fn union_encoding_uses_portable_order_instead_of_local_type_ids() {
    let mut encodings = Vec::new();
    for names in [["probe::A", "probe::B"], ["probe::B", "probe::A"]] {
        let mut types = CanonicalTypeStore::new().unwrap();
        let mut members = Vec::new();
        for name in names {
            let head = NominalHead::Declaration(external(name));
            types.register_nominal_head(head.clone(), 0).unwrap();
            members.push(types.nominal(head, []).unwrap());
        }
        let union = types.error_union(members).unwrap();
        let stable = StableType::from_type_with(union, &BTreeMap::new(), &types, &mut |target| {
            Ok(stable_external(target))
        })
        .unwrap();

        let StableType::ErrorUnion(members) = &stable else {
            panic!("two different nominal members remain a union");
        };
        assert_eq!(members.len(), 2);
        assert!(members[0] < members[1]);
        encodings.push(encode(&stable));
    }
    assert_eq!(encodings[0], encodings[1]);
}

#[test]
fn projection_targets_preserve_callback_and_failure_order() {
    let expected = [
        external("probe::Base"),
        external("probe::Trait"),
        external("probe::First"),
        external("probe::Second"),
        external("probe::Trait::Item"),
    ];
    let mut types = CanonicalTypeStore::new().unwrap();
    let base = types.self_type(expected[0].clone()).unwrap();
    let first = types.self_type(expected[2].clone()).unwrap();
    let second = types.self_type(expected[3].clone()).unwrap();
    let arguments = types.intern_substitution([first, second]).unwrap();
    let projection = types
        .associated_projection(AssociatedProjection::new(
            base,
            TraitRef::new(expected[1].clone(), arguments),
            expected[4].clone(),
        ))
        .unwrap();

    for fail_at in 0..=expected.len() {
        let mut observed = Vec::new();
        let result =
            StableType::from_type_with(projection, &BTreeMap::new(), &types, &mut |target| {
                observed.push(target.clone());
                if fail_at == observed.len() {
                    Err(StableDeclarationError::GenericParameterOverflow(7))
                } else {
                    Ok(stable_external(target))
                }
            });
        if fail_at == 0 {
            assert!(matches!(
                result,
                Ok(StableType::AssociatedProjection { .. })
            ));
            assert_eq!(observed, expected);
        } else {
            assert!(matches!(
                result,
                Err(StableDeclarationError::GenericParameterOverflow(7))
            ));
            assert_eq!(observed, expected[..fail_at]);
        }
    }
}

#[test]
fn unbound_generic_stops_before_later_targets() {
    let mut types = CanonicalTypeStore::new().unwrap();
    let parameter = GenericParameterId::Trusted {
        owner: ExternalDefId::new("probe::Generic").unwrap(),
        index: 0,
    };
    let first = types.self_type(external("probe::First")).unwrap();
    let generic = types.generic_parameter(parameter.clone()).unwrap();
    let last = types.self_type(external("probe::Last")).unwrap();
    let tuple = types.tuple([types.never(), first, generic, last]).unwrap();
    let mut observed = Vec::new();
    let result = StableType::from_type_with(tuple, &BTreeMap::new(), &types, &mut |target| {
        observed.push(target.clone());
        Ok(stable_external(target))
    });
    assert!(matches!(
        result,
        Err(StableDeclarationError::UnboundGenericParameter(found)) if found == parameter
    ));
    assert_eq!(observed, [external("probe::First")]);

    let stable = StableType::from_type_with(
        tuple,
        &BTreeMap::from([(parameter, 3)]),
        &types,
        &mut |target| Ok(stable_external(target)),
    )
    .unwrap();
    let StableType::Tuple(elements) = stable else {
        panic!("tuple shape is preserved");
    };
    assert_eq!(elements[0], StableType::Never);
    assert_eq!(elements[2], StableType::GenericParameter(3));
}

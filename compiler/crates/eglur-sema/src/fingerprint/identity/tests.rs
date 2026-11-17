use super::*;
use crate::{AssociatedProjection, PrimitiveType};

pub(super) mod generic_ordinals;

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
fn intrinsic_binding_keys_encode_their_identity_without_an_external_declaration() {
    let keys = StableDeclarationKeys {
        keys: BTreeMap::new(),
    };
    for intrinsic in [
        IntrinsicNominal::Own,
        IntrinsicNominal::Box,
        IntrinsicNominal::Opt,
        IntrinsicNominal::Res,
    ] {
        let canonical = keys.target(&CanonicalTarget::Intrinsic(intrinsic)).unwrap();
        assert_eq!(canonical, StableTargetKey::Intrinsic(intrinsic));
        let mut actual = CanonicalFingerprintBuilder::new(FingerprintDomain::Interface);
        canonical.write_to(&mut actual);
        let mut expected = CanonicalFingerprintBuilder::new(FingerprintDomain::Interface);
        expected.write_u8(3);
        expected.write_str(intrinsic.spelling());
        assert_eq!(actual.finish(), expected.finish());
        let mut fabricated = CanonicalFingerprintBuilder::new(FingerprintDomain::Interface);
        StableTargetKey::External(
            ExternalDefId::new(format!("std::prelude::{}", intrinsic.spelling())).unwrap(),
        )
        .write_to(&mut fabricated);
        let mut actual = CanonicalFingerprintBuilder::new(FingerprintDomain::Interface);
        canonical.write_to(&mut actual);
        assert_ne!(actual.finish(), fabricated.finish());
    }
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

        let stable =
            StableType::from_type_with(ty, &BinderContext::empty(), &types, &mut |target| {
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
    let stable =
        StableType::from_type_with(callable, &BinderContext::empty(), &types, &mut |target| {
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
        let stable =
            StableType::from_type_with(union, &BinderContext::empty(), &types, &mut |target| {
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
        let result = StableType::from_type_with(
            projection,
            &BinderContext::empty(),
            &types,
            &mut |target| {
                observed.push(target.clone());
                if fail_at == observed.len() {
                    Err(StableDeclarationError::GenericParameterOverflow(7))
                } else {
                    Ok(stable_external(target))
                }
            },
        );
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
    let result =
        StableType::from_type_with(tuple, &BinderContext::empty(), &types, &mut |target| {
            observed.push(target.clone());
            Ok(stable_external(target))
        });
    assert!(matches!(
        result,
        Err(StableDeclarationError::UnboundGenericParameter(found)) if found == parameter
    ));
    assert_eq!(observed, [external("probe::First")]);

    let (collection, signatures, diagnostics) =
        generic_ordinals::headers("pub fn bound[A, B, C, D](value: Bool) -> Bool = value\n");
    assert!(diagnostics.diagnostics().is_empty());
    let context = DeclarationBinderContexts::new(&collection, &signatures)
        .context(generic_ordinals::owner(&collection, "bound"))
        .unwrap();
    let parameter = GenericParameterId::Source(generic_ordinals::owner(&collection, "D"));
    let generic = types.generic_parameter(parameter).unwrap();
    let tuple = types.tuple([types.never(), first, generic, last]).unwrap();
    let stable = StableType::from_type_with(tuple, &context, &types, &mut |target| {
        Ok(stable_external(target))
    })
    .unwrap();
    let StableType::Tuple(elements) = stable else {
        panic!("tuple shape is preserved");
    };
    assert_eq!(elements[0], StableType::Never);
    assert_eq!(elements[2], StableType::GenericParameter(3));
}

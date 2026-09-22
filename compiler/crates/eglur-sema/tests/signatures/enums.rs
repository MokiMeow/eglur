use super::*;
use eglur_sema::{ExternalEnvironmentError, PrimitiveType, TrustedEnumField};

fn id(value: &str) -> ExternalDefId {
    ExternalDefId::new(value).unwrap()
}

fn name(value: &str) -> NameKey {
    NameKey::new(value, false).unwrap()
}

fn variant(identity: &str, spelling: &str, payload: TrustedEnumPayload) -> TrustedEnumVariant {
    TrustedEnumVariant {
        id: id(identity),
        name: name(spelling),
        payload,
    }
}

fn enum_signature(variants: Vec<TrustedEnumVariant>, arity: u32) -> TrustedDeclarationSignature {
    TrustedDeclarationSignature::new(
        id("test::Choice"),
        TrustedSignatureKind::Enum {
            generic_parameters: (0..arity)
                .map(|index| TrustedGenericParameter {
                    name: format!("T{index}").into(),
                    bounds: Box::new([]),
                })
                .collect(),
            variants: variants.into_boxed_slice(),
        },
    )
}

fn names(kind: DeclarationKind) -> ExternalNameEnvironment {
    let module = ModuleAddress::new([Box::<str>::from("test")]).unwrap();
    ExternalNameEnvironment::new(
        [module.clone()],
        [ExternalDeclaration::new(id("test::Choice"), module, name("Choice"), kind).unwrap()],
        [id("test::Choice")],
    )
    .unwrap()
}

#[test]
fn trusted_enum_retains_order_identity_payload_shape_names_and_generic_owner() {
    let field = |identity: &str, spelling: &str, ty| TrustedEnumField {
        id: id(identity),
        name: name(spelling),
        ty,
    };
    let variants = vec![
        variant("member::z", "Stop", TrustedEnumPayload::Absent),
        variant(
            "member::y",
            "Empty",
            TrustedEnumPayload::Tuple(Box::new([])),
        ),
        variant(
            "member::x",
            "Value",
            TrustedEnumPayload::Tuple(Box::new([
                TrustedTypeExpr::GenericParameter(0),
                TrustedTypeExpr::Primitive(PrimitiveType::Bool),
            ])),
        ),
        variant(
            "member::w",
            "Record",
            TrustedEnumPayload::Named(Box::new([
                field(
                    "field::z",
                    "count",
                    TrustedTypeExpr::Primitive(PrimitiveType::I64),
                ),
                field("field::a", "value", TrustedTypeExpr::GenericParameter(0)),
            ])),
        ),
    ];
    let external = names(DeclarationKind::Enum);
    let trusted = TrustedSignatureEnvironment::new([enum_signature(variants.clone(), 1)]).unwrap();
    let analysis = analyze(
        "",
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
    );
    assert!(diagnostic_codes(&analysis.diagnostics).is_empty());
    let signatures = analysis.signatures.unwrap();
    let owner = CanonicalTarget::External(id("test::Choice"));
    let FactState::Known(DeclarationSignature::Enum {
        generic_parameters,
        variants: members,
    }) = signatures.declaration(&owner)
    else {
        panic!("trusted enum must have a canonical declaration");
    };
    assert_eq!(
        generic_parameters.as_ref(),
        &[GenericParameterId::Trusted {
            owner: id("test::Choice"),
            index: 0
        }]
    );
    assert_eq!(
        members.as_ref(),
        variants
            .iter()
            .map(|variant| CanonicalTarget::External(variant.id.clone()))
            .collect::<Vec<_>>()
    );

    for (index, member) in members.iter().enumerate() {
        let FactState::Known(DeclarationSignature::EnumVariant {
            owner: actual_owner,
            name: actual_name,
            payload,
        }) = signatures.declaration(member)
        else {
            panic!("trusted variant must normalize into the canonical graph");
        };
        assert_eq!(actual_owner, &owner);
        assert_eq!(actual_name, &variants[index].name);
        match (index, payload) {
            (0, EnumVariantPayloadSignature::Absent) => {}
            (1, EnumVariantPayloadSignature::Tuple(types)) => assert!(types.is_empty()),
            (2, EnumVariantPayloadSignature::Tuple(types)) => {
                assert_eq!(types.len(), 2);
                assert_eq!(
                    signatures.types().kind(types[0]).unwrap(),
                    &TypeKind::GenericParameter(generic_parameters[0].clone())
                );
                assert_eq!(
                    signatures.types().kind(types[1]).unwrap(),
                    &TypeKind::Primitive(PrimitiveType::Bool)
                );
            }
            (3, EnumVariantPayloadSignature::Named(fields)) => {
                assert_eq!(
                    fields.as_ref(),
                    &[
                        CanonicalTarget::External(id("field::z")),
                        CanonicalTarget::External(id("field::a"))
                    ]
                );
                for (target, spelling, expected) in [
                    (&fields[0], "count", TypeKind::Primitive(PrimitiveType::I64)),
                    (
                        &fields[1],
                        "value",
                        TypeKind::GenericParameter(generic_parameters[0].clone()),
                    ),
                ] {
                    let FactState::Known(DeclarationSignature::Field {
                        name: field_name,
                        ty,
                    }) = signatures.declaration(target)
                    else {
                        panic!("named payload field must normalize into the canonical graph");
                    };
                    assert_eq!(field_name, &name(spelling));
                    assert_eq!(signatures.types().kind(*ty).unwrap(), &expected);
                }
            }
            _ => panic!("payload shape or order changed"),
        }
    }
}

#[test]
fn trusted_enum_rejects_member_identity_collisions_and_duplicate_or_bang_names() {
    let stop = variant("test::stop", "Stop", TrustedEnumPayload::Absent);
    for other in [
        variant("test::stop", "Different", TrustedEnumPayload::Absent),
        variant("test::Choice", "Different", TrustedEnumPayload::Absent),
    ] {
        assert!(matches!(
            TrustedSignatureEnvironment::new([enum_signature(vec![stop.clone(), other], 0)]),
            Err(TrustedSignatureError::DuplicateDeclaration(_))
        ));
    }
    let mut bang = variant("test::bang", "Bang", TrustedEnumPayload::Absent);
    bang.name = NameKey::new("Bang", true).unwrap();
    for other in [
        variant("test::other", "Stop", TrustedEnumPayload::Absent),
        bang,
    ] {
        assert!(matches!(
            TrustedSignatureEnvironment::new([enum_signature(vec![stop.clone(), other], 0)]),
            Err(TrustedSignatureError::InvalidEnumMember { .. })
        ));
    }

    let base = TrustedEnumField {
        id: id("test::field"),
        name: name("value"),
        ty: TrustedTypeExpr::Never,
    };
    for (identity, spelling, bang) in [
        ("test::field", "other", false),
        ("test::record", "other", false),
        ("test::Choice", "other", false),
        ("test::other", "value", false),
        ("test::other", "other", true),
    ] {
        let other = TrustedEnumField {
            id: id(identity),
            name: NameKey::new(spelling, bang).unwrap(),
            ty: TrustedTypeExpr::Never,
        };
        let record = variant(
            "test::record",
            "Record",
            TrustedEnumPayload::Named(Box::new([base.clone(), other])),
        );
        let result = TrustedSignatureEnvironment::new([enum_signature(vec![record], 0)]);
        if spelling == "value" || bang {
            assert!(matches!(
                result,
                Err(TrustedSignatureError::InvalidEnumMember { .. })
            ));
        } else {
            assert!(matches!(
                result,
                Err(TrustedSignatureError::DuplicateDeclaration(_))
            ));
        }
    }

    let another = TrustedDeclarationSignature::new(
        id("test::Other"),
        TrustedSignatureKind::Enum {
            generic_parameters: Box::new([]),
            variants: Box::new([stop.clone()]),
        },
    );
    assert!(matches!(
        TrustedSignatureEnvironment::new([enum_signature(vec![stop], 0), another]),
        Err(TrustedSignatureError::DuplicateDeclaration(_))
    ));
}

#[test]
fn enum_payload_types_use_the_enum_generic_scope_and_ordinary_type_positions() {
    let external = names(DeclarationKind::Enum);
    for (ty, failure) in [
        (TrustedTypeExpr::GenericParameter(1), "generic"),
        (
            TrustedTypeExpr::Nominal {
                head: id("test::Missing"),
                arguments: Box::new([]),
            },
            "head",
        ),
        (
            TrustedTypeExpr::Intrinsic {
                head: IntrinsicNominal::Opt,
                arguments: Box::new([TrustedTypeExpr::Reference {
                    mutability: ReferenceMutability::Shared,
                    referent: Box::new(TrustedTypeExpr::Never),
                }]),
            },
            "position",
        ),
        (
            TrustedTypeExpr::Intrinsic {
                head: IntrinsicNominal::Own,
                arguments: Box::new([TrustedTypeExpr::Never]),
            },
            "position",
        ),
        (
            TrustedTypeExpr::ErrorUnion(Box::new([TrustedTypeExpr::Never])),
            "position",
        ),
    ] {
        for named in [false, true] {
            let payload = if named {
                TrustedEnumPayload::Named(Box::new([TrustedEnumField {
                    id: id("test::field"),
                    name: name("value"),
                    ty: ty.clone(),
                }]))
            } else {
                TrustedEnumPayload::Tuple(Box::new([ty.clone()]))
            };
            let trusted = TrustedSignatureEnvironment::new([enum_signature(
                vec![variant("test::value", "Value", payload)],
                1,
            )])
            .unwrap();
            let Err(SignatureResolutionError::Trusted(error)) = try_analyze(
                "",
                &CapabilitySet::signatures(),
                Some(&external),
                Some(&trusted),
            ) else {
                panic!("invalid enum payload type was accepted: {ty:?}, named={named}");
            };
            assert!(matches!(
                (failure, error),
                (
                    "generic",
                    TrustedSignatureError::GenericParameterOutOfRange { .. }
                ) | ("head", TrustedSignatureError::UnknownTypeHead(_))
                    | (
                        "position",
                        TrustedSignatureError::InvalidTypePosition { .. }
                    )
            ));
        }
    }
}

#[test]
fn enum_member_spellings_must_be_source_identifiers() {
    for spelling in [
        "Bad Name",
        "123",
        "match",
        "_",
        "own",
        "drop_own",
        "a-b",
        "Color::Red",
        "\u{03bb}",
        "a\n",
        "a\0",
    ] {
        let invalid = variant("test::variant", spelling, TrustedEnumPayload::Absent);
        assert!(
            matches!(
                TrustedSignatureEnvironment::new([enum_signature(vec![invalid], 0)]),
                Err(TrustedSignatureError::InvalidEnumMember { .. })
            ),
            "variant {spelling:?}"
        );

        let payload = TrustedEnumPayload::Named(Box::new([TrustedEnumField {
            id: id("test::field"),
            name: name(spelling),
            ty: TrustedTypeExpr::Never,
        }]));
        let invalid = variant("test::variant", "Value", payload);
        assert!(
            matches!(
                TrustedSignatureEnvironment::new([enum_signature(vec![invalid], 0)]),
                Err(TrustedSignatureError::InvalidEnumMember { .. })
            ),
            "field {spelling:?}"
        );
    }

    for spelling in ["Some", "None", "Ok", "Err"] {
        let invalid = variant("test::variant", spelling, TrustedEnumPayload::Absent);
        assert!(
            matches!(
                TrustedSignatureEnvironment::new([enum_signature(vec![invalid], 0)]),
                Err(TrustedSignatureError::InvalidEnumMember { .. })
            ),
            "reserved constructor {spelling}"
        );
    }

    for spelling in [
        "_name", "Value2", "self", "super", "closed", "view", "Fn", "Self", "Never", "Union",
    ] {
        let valid = variant(
            "test::variant",
            spelling,
            TrustedEnumPayload::Named(Box::new([
                TrustedEnumField {
                    id: id("test::field"),
                    name: name(spelling),
                    ty: TrustedTypeExpr::Never,
                },
                TrustedEnumField {
                    id: id("test::residual"),
                    name: name("Some"),
                    ty: TrustedTypeExpr::Never,
                },
            ])),
        );
        assert!(
            TrustedSignatureEnvironment::new([enum_signature(vec![valid], 0)]).is_ok(),
            "{spelling}"
        );
    }
}

#[test]
fn enum_metadata_cannot_be_replaced_by_nominal_or_inhabitance_summaries() {
    for (kind, signature) in [
        (
            DeclarationKind::Enum,
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        ),
        (
            DeclarationKind::OpaqueType,
            TrustedSignatureKind::Enum {
                generic_parameters: Box::new([]),
                variants: Box::new([]),
            },
        ),
    ] {
        let external = names(kind);
        let trusted = TrustedSignatureEnvironment::new([TrustedDeclarationSignature::new(
            id("test::Choice"),
            signature,
        )])
        .unwrap();
        assert!(matches!(
            try_analyze(
                "",
                &CapabilitySet::signatures(),
                Some(&external),
                Some(&trusted)
            ),
            Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::DeclarationKindMismatch(_)
            ))
        ));
    }

    let external = names(DeclarationKind::Enum);
    let trusted = TrustedSignatureEnvironment::new([enum_signature(vec![], 0)]).unwrap();
    let inhabitance = TrustedInhabitanceEnvironment::new([TrustedInhabitanceFact::new(
        id("test::Choice"),
        TrustedNominalInhabitance::Inhabited,
    )])
    .unwrap();
    assert!(matches!(
        try_analyze_with_inhabitance(
            "",
            &CapabilitySet::signatures(),
            Some(&external),
            Some(&trusted),
            Some(&inhabitance),
            None
        ),
        Err(SignatureResolutionError::TrustedInhabitanceOnNonNominal(_))
    ));
}

#[test]
fn trusted_enum_inhabitance_is_derived_from_its_payloads() {
    for (variants, expected) in [
        (vec![], Inhabitance::Uninhabited),
        (
            vec![variant("test::stop", "Stop", TrustedEnumPayload::Absent)],
            Inhabitance::Inhabited,
        ),
        (
            vec![variant(
                "test::empty",
                "Empty",
                TrustedEnumPayload::Tuple(Box::new([])),
            )],
            Inhabitance::Inhabited,
        ),
        (
            vec![variant(
                "test::never",
                "NeverValue",
                TrustedEnumPayload::Tuple(Box::new([TrustedTypeExpr::Never])),
            )],
            Inhabitance::Uninhabited,
        ),
        (
            vec![variant(
                "test::value",
                "Value",
                TrustedEnumPayload::Tuple(Box::new([TrustedTypeExpr::GenericParameter(0)])),
            )],
            Inhabitance::Unknown,
        ),
    ] {
        let external = names(DeclarationKind::Enum);
        let trusted = TrustedSignatureEnvironment::new([enum_signature(variants, 1)]).unwrap();
        let mut analysis = analyze(
            "fn f[T](value: Choice[T]) -> Unit = ()\n",
            &CapabilitySet::signatures(),
            Some(&external),
            Some(&trusted),
        );
        assert!(diagnostic_codes(&analysis.diagnostics).is_empty());
        let mut signatures = analysis.signatures.take().unwrap();
        let function = source_def(&analysis.declarations, "f", DeclarationKind::Function);
        let FactState::Known(DeclarationSignature::Callable(callable)) =
            signatures.source_declaration(function)
        else {
            panic!("parameter type must resolve");
        };
        let ty = signatures.callable(*callable).unwrap().parameters()[0].ty();
        assert_eq!(
            analysis
                .inhabitance
                .as_mut()
                .unwrap()
                .classify(&mut signatures, ty)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn nested_enum_metadata_does_not_introduce_standalone_external_names() {
    let module = ModuleAddress::new([Box::<str>::from("test")]).unwrap();
    for kind in [
        DeclarationKind::EnumVariant,
        DeclarationKind::EnumVariantField,
    ] {
        assert!(matches!(
            ExternalDeclaration::new(id("test::member"), module.clone(), name("member"), kind),
            Err(ExternalEnvironmentError::UnnameableDeclarationKind(actual)) if actual == kind
        ));
    }
}

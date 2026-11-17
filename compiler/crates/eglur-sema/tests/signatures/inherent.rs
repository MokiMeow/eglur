use super::*;
use eglur_hir::Visibility;
use eglur_sema::{
    ConcreteOwnerHead, DeclarationSignatures, InherentMemberForm, PrimitiveType,
    TrustedInherentMember, TrustedInherentOwner, TrustedInherentOwnerHead,
};

fn id(text: &str) -> ExternalDefId {
    ExternalDefId::new(text).unwrap()
}

fn scalar() -> TrustedTypeExpr {
    TrustedTypeExpr::Primitive(PrimitiveType::I64)
}

fn applied(head: IntrinsicNominal, argument: TrustedTypeExpr) -> TrustedTypeExpr {
    TrustedTypeExpr::Intrinsic {
        head,
        arguments: Box::new([argument]),
    }
}

fn nominal(head: &str, count: usize) -> TrustedDeclarationSignature {
    TrustedDeclarationSignature::new(
        id(head),
        TrustedSignatureKind::Nominal {
            generic_parameters: generics(count),
        },
    )
}

fn generics(count: usize) -> Box<[TrustedGenericParameter]> {
    (0..count)
        .map(|index| TrustedGenericParameter {
            name: format!("T{index}").into(),
            bounds: Box::new([]),
        })
        .collect()
}

fn method(
    callable: &str,
    name: &str,
    owner_pattern: TrustedTypeExpr,
    count: usize,
    form: InherentMemberForm,
) -> (TrustedDeclarationSignature, TrustedInherentMember) {
    let bang = form == InherentMemberForm::MutableReceiver;
    let parameters = match form {
        InherentMemberForm::Associated => Vec::new(),
        InherentMemberForm::ValueReceiver => vec![TrustedCallableParameter {
            name: Some("self".into()),
            ty: owner_pattern.clone(),
        }],
        InherentMemberForm::SharedReceiver | InherentMemberForm::MutableReceiver => {
            vec![TrustedCallableParameter {
                name: Some("self".into()),
                ty: TrustedTypeExpr::Reference {
                    mutability: if form == InherentMemberForm::SharedReceiver {
                        ReferenceMutability::Shared
                    } else {
                        ReferenceMutability::Mutable
                    },
                    referent: Box::new(owner_pattern.clone()),
                },
            }]
        }
    };
    (
        TrustedDeclarationSignature::new(
            id(callable),
            TrustedSignatureKind::Callable {
                bang,
                generic_parameters: generics(count),
                parameters: parameters.into_boxed_slice(),
                result: TrustedTypeExpr::Primitive(PrimitiveType::Unit),
                requirements: Box::new([]),
            },
        ),
        TrustedInherentMember {
            callable: id(callable),
            name: NameKey::new(name, bang).unwrap(),
            owner_pattern,
            owner_parameter_count: count,
            form,
            visibility: Visibility::Public,
            overload: false,
        },
    )
}

fn owner(
    head: TrustedInherentOwnerHead,
    members: Vec<TrustedInherentMember>,
) -> TrustedInherentOwner {
    TrustedInherentOwner {
        head,
        members: members.into_boxed_slice(),
    }
}

fn canonicalize(
    trusted: &TrustedSignatureEnvironment,
) -> Result<DeclarationSignatures, SignatureResolutionError> {
    let module = ModuleAddress::new(["std".into(), "fixture".into()]).unwrap();
    let names = ExternalNameEnvironment::new(
        [module.clone()],
        trusted
            .declarations()
            .enumerate()
            .filter_map(|(index, declaration)| {
                let kind = match declaration.kind() {
                    TrustedSignatureKind::Nominal { .. } => DeclarationKind::OpaqueType,
                    TrustedSignatureKind::Trait { .. } => DeclarationKind::Trait,
                    _ => return None,
                };
                Some(
                    ExternalDeclaration::new(
                        declaration.id().clone(),
                        module.clone(),
                        NameKey::new(format!("Type{index}"), false).unwrap(),
                        kind,
                    )
                    .unwrap(),
                )
            }),
        [],
    )
    .unwrap();
    let analysis = try_analyze(
        "fn local() -> Unit = ()\n",
        &CapabilitySet::signatures(),
        Some(&names),
        Some(trusted),
    )?;
    assert!(analysis.diagnostics.diagnostics().is_empty());
    Ok(analysis.signatures.unwrap())
}

#[test]
fn ordinary_owner_kinds_preserve_complete_patterns_and_callable_identity() {
    let root = TrustedTypeExpr::Nominal {
        head: id("fixture::Reader"),
        arguments: Box::new([]),
    };
    for (head, pattern, count) in [
        (
            TrustedInherentOwnerHead::Primitive(PrimitiveType::Str),
            TrustedTypeExpr::Primitive(PrimitiveType::Str),
            0,
        ),
        (
            TrustedInherentOwnerHead::Nominal(id("fixture::Reader")),
            root.clone(),
            0,
        ),
        (
            TrustedInherentOwnerHead::Intrinsic(IntrinsicNominal::Box),
            applied(
                IntrinsicNominal::Box,
                applied(IntrinsicNominal::Box, TrustedTypeExpr::GenericParameter(0)),
            ),
            1,
        ),
        (
            TrustedInherentOwnerHead::Intrinsic(IntrinsicNominal::Own),
            applied(IntrinsicNominal::Own, root.clone()),
            0,
        ),
    ] {
        let (callable, member) = method(
            "fixture::member",
            "read",
            pattern,
            count,
            InherentMemberForm::SharedReceiver,
        );
        let trusted = TrustedSignatureEnvironment::with_inherent_owners(
            [nominal("fixture::Reader", 0), callable],
            [owner(head.clone(), vec![member])],
        )
        .unwrap();
        assert!(trusted.is_hidden_callable(&id("fixture::member")));
        let canonical = canonicalize(&trusted).unwrap();
        let key = match head {
            TrustedInherentOwnerHead::Primitive(ty) => ConcreteOwnerHead::Primitive(ty),
            TrustedInherentOwnerHead::Intrinsic(head) => {
                ConcreteOwnerHead::Nominal(NominalHead::Intrinsic(head))
            }
            TrustedInherentOwnerHead::Nominal(head) => ConcreteOwnerHead::Nominal(
                NominalHead::Declaration(CanonicalTarget::External(head)),
            ),
        };
        let family = canonical
            .trusted_inherent_owner(&key)
            .unwrap()
            .family(&NameKey::new("read", false).unwrap())
            .unwrap();
        let member = &family.members()[0];
        let callable = canonical.callable(member.callable()).unwrap();
        assert_eq!(
            callable.declaration(),
            &CanonicalTarget::External(id("fixture::member"))
        );
        assert_eq!(member.owner_parameter_count(), count);
        assert_eq!(member.form(), InherentMemberForm::SharedReceiver);
        assert_eq!(member.visibility(), Visibility::Public);
        assert!(family.is_public());
        assert!(
            matches!(canonical.types().kind(callable.parameters()[0].ty()).unwrap(), TypeKind::Reference { referent, .. } if *referent == member.owner_pattern())
        );
        if let ConcreteOwnerHead::Primitive(primitive) = key {
            assert_eq!(
                canonical.types().kind(member.owner_pattern()).unwrap(),
                &TypeKind::Primitive(primitive)
            );
        }
    }
}

#[test]
fn empty_inventory_is_distinct_from_unavailable_membership() {
    let head = TrustedInherentOwnerHead::Primitive(PrimitiveType::Str);
    let absent = TrustedSignatureEnvironment::new([]).unwrap();
    let empty =
        TrustedSignatureEnvironment::with_inherent_owners([], [owner(head.clone(), vec![])])
            .unwrap();
    assert!(absent.inherent_owner(&head).is_none());
    assert!(empty.inherent_owner(&head).unwrap().members.is_empty());
    let key = ConcreteOwnerHead::Primitive(PrimitiveType::Str);
    assert!(
        canonicalize(&absent)
            .unwrap()
            .trusted_inherent_owner(&key)
            .is_none()
    );
    let canonical = canonicalize(&empty).unwrap();
    let inventory = canonical.trusted_inherent_owner(&key).unwrap();
    assert!(inventory.families().next().is_none());
    assert!(
        inventory
            .family(&NameKey::new("missing", false).unwrap())
            .is_none()
    );
}

#[test]
fn all_receiver_forms_and_associated_functions_have_checked_contracts() {
    for form in [
        InherentMemberForm::Associated,
        InherentMemberForm::ValueReceiver,
        InherentMemberForm::SharedReceiver,
        InherentMemberForm::MutableReceiver,
    ] {
        let (callable, member) = method("fixture::method", "act", scalar(), 0, form);
        let bang = form == InherentMemberForm::MutableReceiver;
        let trusted = TrustedSignatureEnvironment::with_inherent_owners(
            [callable],
            [owner(
                TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
                vec![member],
            )],
        )
        .unwrap();
        let canonical = canonicalize(&trusted).unwrap();
        let member = &canonical
            .trusted_inherent_owner(&ConcreteOwnerHead::Primitive(PrimitiveType::I64))
            .unwrap()
            .family(&NameKey::new("act", bang).unwrap())
            .unwrap()
            .members()[0];
        assert_eq!(member.form(), form);
        assert_eq!(
            canonical.callable(member.callable()).unwrap().is_bang(),
            bang
        );
    }
}

#[test]
fn owner_and_method_binders_retain_one_callable_context() {
    let pattern = applied(IntrinsicNominal::Box, TrustedTypeExpr::GenericParameter(0));
    let (callable, mut member) = method(
        "fixture::map",
        "map",
        pattern,
        2,
        InherentMemberForm::ValueReceiver,
    );
    member.owner_parameter_count = 1;
    let trusted = TrustedSignatureEnvironment::with_inherent_owners(
        [callable],
        [owner(
            TrustedInherentOwnerHead::Intrinsic(IntrinsicNominal::Box),
            vec![member],
        )],
    )
    .unwrap();
    let canonical = canonicalize(&trusted).unwrap();
    let member = &canonical
        .trusted_inherent_owner(&ConcreteOwnerHead::Nominal(NominalHead::Intrinsic(
            IntrinsicNominal::Box,
        )))
        .unwrap()
        .family(&NameKey::new("map", false).unwrap())
        .unwrap()
        .members()[0];
    let callable = canonical.callable(member.callable()).unwrap();
    assert_eq!(member.owner_parameter_count(), 1);
    assert_eq!(callable.generic_parameters().len(), 2);
    let TypeKind::Nominal { substitution, .. } =
        canonical.types().kind(member.owner_pattern()).unwrap()
    else {
        panic!("expected applied owner")
    };
    let argument = canonical.types().substitution(*substitution).unwrap()[0];
    assert_eq!(
        canonical.types().kind(argument).unwrap(),
        &TypeKind::GenericParameter(callable.generic_parameters()[0].clone())
    );
}

#[test]
fn generic_nominal_members_bind_the_complete_owner_in_the_callable_context() {
    let pattern = TrustedTypeExpr::Nominal {
        head: id("fixture::Container"),
        arguments: Box::new([TrustedTypeExpr::GenericParameter(0)]),
    };
    let (callable, mut member) = method(
        "fixture::member",
        "read",
        pattern,
        2,
        InherentMemberForm::ValueReceiver,
    );
    member.owner_parameter_count = 1;
    let callable = change_callable(&callable, |kind| {
        let TrustedSignatureKind::Callable {
            parameters, result, ..
        } = kind
        else {
            unreachable!()
        };
        *parameters = Box::new([
            parameters[0].clone(),
            TrustedCallableParameter {
                name: Some("value".into()),
                ty: TrustedTypeExpr::GenericParameter(1),
            },
        ]);
        *result = TrustedTypeExpr::GenericParameter(1);
    });
    let trusted = TrustedSignatureEnvironment::with_inherent_owners(
        [nominal("fixture::Container", 1), callable],
        [owner(
            TrustedInherentOwnerHead::Nominal(id("fixture::Container")),
            vec![member],
        )],
    )
    .unwrap();
    let canonical = canonicalize(&trusted).unwrap();
    let head = ConcreteOwnerHead::Nominal(NominalHead::Declaration(CanonicalTarget::External(id(
        "fixture::Container",
    ))));
    let member = &canonical
        .trusted_inherent_owner(&head)
        .unwrap()
        .family(&NameKey::new("read", false).unwrap())
        .unwrap()
        .members()[0];
    let callable = canonical.callable(member.callable()).unwrap();
    assert_eq!(member.owner_pattern(), callable.parameters()[0].ty());
    assert_eq!(member.owner_parameter_count(), 1);
    assert_eq!(
        canonical.types().kind(callable.result()).unwrap(),
        &TypeKind::GenericParameter(callable.generic_parameters()[1].clone())
    );
    assert_eq!(callable.result(), callable.parameters()[1].ty());
}

#[test]
fn explicit_overload_family_order_and_private_members_are_preserved() {
    let (a, mut ma) = method(
        "fixture::a",
        "act",
        scalar(),
        0,
        InherentMemberForm::ValueReceiver,
    );
    let (b, mut mb) = method(
        "fixture::b",
        "act",
        scalar(),
        0,
        InherentMemberForm::SharedReceiver,
    );
    ma.overload = true;
    mb.overload = true;
    mb.visibility = Visibility::Private;
    let head = TrustedInherentOwnerHead::Primitive(PrimitiveType::I64);
    let first = TrustedSignatureEnvironment::with_inherent_owners(
        [b.clone(), a.clone()],
        [owner(head.clone(), vec![mb.clone(), ma.clone()])],
    )
    .unwrap();
    let second = TrustedSignatureEnvironment::with_inherent_owners(
        [a, b],
        [owner(head.clone(), vec![ma, mb])],
    )
    .unwrap();
    assert_eq!(first.inherent_owner(&head), second.inherent_owner(&head));
    let left = canonicalize(&first).unwrap();
    let right = canonicalize(&second).unwrap();
    let key = ConcreteOwnerHead::Primitive(PrimitiveType::I64);
    assert_eq!(
        left.trusted_inherent_owner(&key),
        right.trusted_inherent_owner(&key)
    );
    let family = left
        .trusted_inherent_owner(&key)
        .unwrap()
        .family(&NameKey::new("act", false).unwrap())
        .unwrap();
    assert!(!family.is_public());
    let identities = family
        .members()
        .iter()
        .map(|member| {
            left.callable(member.callable())
                .unwrap()
                .declaration()
                .clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        [
            CanonicalTarget::External(id("fixture::a")),
            CanonicalTarget::External(id("fixture::b"))
        ]
    );
}

fn change_callable(
    declaration: &TrustedDeclarationSignature,
    change: impl FnOnce(&mut TrustedSignatureKind),
) -> TrustedDeclarationSignature {
    let mut kind = declaration.kind().clone();
    change(&mut kind);
    TrustedDeclarationSignature::new(declaration.id().clone(), kind)
}

fn assert_invalid_member(result: Result<TrustedSignatureEnvironment, TrustedSignatureError>) {
    assert!(matches!(
        result,
        Err(TrustedSignatureError::InvalidInherentMember { .. })
    ));
}

#[test]
fn owner_and_callable_identity_admission_rejects_unknown_wrong_and_duplicate_relations() {
    let (callable, member) = method(
        "fixture::member",
        "read",
        scalar(),
        0,
        InherentMemberForm::ValueReceiver,
    );
    let head = TrustedInherentOwnerHead::Primitive(PrimitiveType::I64);
    let input = owner(head.clone(), vec![member.clone()]);
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [],
        [input.clone()],
    ));
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [nominal("fixture::member", 0)],
        [input.clone()],
    ));
    assert!(matches!(
        TrustedSignatureEnvironment::with_inherent_owners(
            [callable.clone()],
            [input.clone(), input]
        ),
        Err(TrustedSignatureError::DuplicateInherentOwner(_))
    ));
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [callable.clone()],
        [owner(head, vec![member.clone(), member.clone()])],
    ));
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [callable.clone()],
        [owner(
            TrustedInherentOwnerHead::Primitive(PrimitiveType::Str),
            vec![member],
        )],
    ));
    for declaration in [
        None,
        Some(callable),
        Some(TrustedDeclarationSignature::new(
            id("fixture::member"),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        )),
    ] {
        let result = TrustedSignatureEnvironment::with_inherent_owners(
            declaration,
            [owner(
                TrustedInherentOwnerHead::Nominal(id("fixture::member")),
                vec![],
            )],
        );
        assert!(matches!(
            result,
            Err(TrustedSignatureError::InvalidInherentOwner { .. })
        ));
    }
}

#[test]
fn trait_members_cannot_acquire_a_second_ordinary_owner() {
    let trait_id = id("fixture::Trait");
    let (callable, member) = method(
        "fixture::member",
        "read",
        scalar(),
        0,
        InherentMemberForm::ValueReceiver,
    );
    let trait_declaration = TrustedDeclarationSignature::new(
        trait_id,
        TrustedSignatureKind::Trait {
            closed: false,
            generic_parameters: Box::new([]),
            associated_types: Box::new([]),
        },
    )
    .with_trait_methods([TrustedTraitMethod {
        declaration: member.callable.clone(),
        name: member.name.clone(),
        has_default: true,
    }]);
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [trait_declaration, callable],
        [owner(
            TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
            vec![member],
        )],
    ));
}

#[test]
fn implementation_members_cannot_acquire_an_ordinary_owner() {
    let trait_id = id("fixture::Trait");
    let (trait_callable, _) = method(
        "fixture::trait_member",
        "read",
        TrustedTypeExpr::SelfType(trait_id.clone()),
        0,
        InherentMemberForm::SharedReceiver,
    );
    let (impl_callable, member) = method(
        "fixture::impl_member",
        "read",
        scalar(),
        0,
        InherentMemberForm::SharedReceiver,
    );
    let trait_declaration = TrustedDeclarationSignature::new(
        trait_id.clone(),
        TrustedSignatureKind::Trait {
            closed: false,
            generic_parameters: Box::new([]),
            associated_types: Box::new([]),
        },
    )
    .with_trait_methods([TrustedTraitMethod {
        declaration: id("fixture::trait_member"),
        name: member.name.clone(),
        has_default: false,
    }]);
    let implementation = TrustedDeclarationSignature::new(
        id("fixture::implementation"),
        TrustedSignatureKind::Impl {
            negative: false,
            generic_parameters: Box::new([]),
            trait_ref: TrustedTraitExpr {
                declaration: trait_id,
                arguments: Box::new([]),
            },
            target: scalar(),
            associated_bindings: Box::new([]),
        },
    )
    .with_impl_methods([TrustedImplMethod {
        trait_method: id("fixture::trait_member"),
        callable: member.callable.clone(),
    }]);
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [
            trait_declaration,
            trait_callable,
            implementation,
            impl_callable,
        ],
        [owner(
            TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
            vec![member],
        )],
    ));
}

#[test]
fn method_local_unknown_and_unused_owner_binders_are_rejected() {
    let pattern = applied(IntrinsicNominal::Box, TrustedTypeExpr::GenericParameter(0));
    let (callable, original) = method(
        "fixture::member",
        "read",
        pattern,
        2,
        InherentMemberForm::Associated,
    );
    let cases = [
        (3, TrustedTypeExpr::GenericParameter(0)),
        (1, TrustedTypeExpr::GenericParameter(1)),
        (1, TrustedTypeExpr::GenericParameter(99)),
        (2, TrustedTypeExpr::GenericParameter(0)),
        (0, TrustedTypeExpr::SelfType(id("fixture::Other"))),
        (
            1,
            TrustedTypeExpr::AssociatedProjection {
                base: Box::new(TrustedTypeExpr::GenericParameter(0)),
                trait_declaration: id("fixture::Trait"),
                trait_arguments: Box::new([]),
                associated: id("fixture::Item"),
            },
        ),
    ];
    for (count, argument) in cases {
        let mut member = original.clone();
        member.owner_parameter_count = count;
        member.owner_pattern = applied(IntrinsicNominal::Box, argument);
        assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
            [callable.clone()],
            [owner(
                TrustedInherentOwnerHead::Intrinsic(IntrinsicNominal::Box),
                vec![member],
            )],
        ));
    }
    let mut member = original;
    member.owner_parameter_count = 1;
    let with_bound = change_callable(&callable, |kind| {
        let TrustedSignatureKind::Callable {
            generic_parameters, ..
        } = kind
        else {
            unreachable!()
        };
        generic_parameters[0].bounds = Box::new([TrustedTraitExpr {
            declaration: id("fixture::Trait"),
            arguments: Box::new([TrustedTypeExpr::GenericParameter(1)]),
        }]);
    });
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [with_bound],
        [owner(
            TrustedInherentOwnerHead::Intrinsic(IntrinsicNominal::Box),
            vec![member],
        )],
    ));
}

#[test]
fn repeated_owner_binders_keep_the_same_identity() {
    let pattern = applied(
        IntrinsicNominal::Box,
        TrustedTypeExpr::Tuple(Box::new([
            TrustedTypeExpr::GenericParameter(0),
            TrustedTypeExpr::GenericParameter(0),
        ])),
    );
    let (callable, member) = method(
        "fixture::member",
        "read",
        pattern,
        1,
        InherentMemberForm::ValueReceiver,
    );
    let trusted = TrustedSignatureEnvironment::with_inherent_owners(
        [callable],
        [owner(
            TrustedInherentOwnerHead::Intrinsic(IntrinsicNominal::Box),
            vec![member],
        )],
    )
    .unwrap();
    let canonical = canonicalize(&trusted).unwrap();
    let member = &canonical
        .trusted_inherent_owner(&ConcreteOwnerHead::Nominal(NominalHead::Intrinsic(
            IntrinsicNominal::Box,
        )))
        .unwrap()
        .family(&NameKey::new("read", false).unwrap())
        .unwrap()
        .members()[0];
    let TypeKind::Nominal { substitution, .. } =
        canonical.types().kind(member.owner_pattern()).unwrap()
    else {
        unreachable!()
    };
    let argument = canonical.types().substitution(*substitution).unwrap()[0];
    let TypeKind::Tuple(elements) = canonical.types().kind(argument).unwrap() else {
        unreachable!()
    };
    assert_eq!(elements[0], elements[1]);
}

#[test]
fn bang_receiver_and_explicit_self_contracts_are_admitted_together() {
    let (callable, member) = method(
        "fixture::member",
        "act",
        scalar(),
        0,
        InherentMemberForm::ValueReceiver,
    );
    let head = TrustedInherentOwnerHead::Primitive(PrimitiveType::I64);
    let mut wrong_bang = member.clone();
    wrong_bang.name = NameKey::new("act", true).unwrap();
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [callable.clone()],
        [owner(head.clone(), vec![wrong_bang])],
    ));
    let mut association = member.clone();
    association.form = InherentMemberForm::Associated;
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [callable.clone()],
        [owner(head.clone(), vec![association])],
    ));
    let mut mutable = member.clone();
    mutable.form = InherentMemberForm::MutableReceiver;
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [callable.clone()],
        [owner(head.clone(), vec![mutable])],
    ));
    for change in [0, 1, 2, 3] {
        let invalid = change_callable(&callable, |kind| {
            let TrustedSignatureKind::Callable {
                parameters,
                result,
                requirements,
                ..
            } = kind
            else {
                unreachable!()
            };
            match change {
                0 => parameters[0].name = Some("value".into()),
                1 => *result = TrustedTypeExpr::SelfType(id("fixture::Trait")),
                2 => {
                    *requirements =
                        Box::new([eglur_sema::TrustedCallableRequirement::FixedWidthInteger(
                            TrustedTypeExpr::SelfType(id("fixture::Trait")),
                        )])
                }
                3 => {
                    *parameters = Box::new([
                        TrustedCallableParameter {
                            name: Some("value".into()),
                            ty: scalar(),
                        },
                        parameters[0].clone(),
                    ]);
                }
                _ => unreachable!(),
            }
        });
        assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
            [invalid],
            [owner(head.clone(), vec![member.clone()])],
        ));
    }
}

#[test]
fn missing_overload_marker_is_rejected_without_choosing_a_candidate() {
    let (a, ma) = method(
        "fixture::a",
        "act",
        scalar(),
        0,
        InherentMemberForm::ValueReceiver,
    );
    let (b, mut mb) = method(
        "fixture::b",
        "act",
        scalar(),
        0,
        InherentMemberForm::SharedReceiver,
    );
    mb.overload = true;
    assert_invalid_member(TrustedSignatureEnvironment::with_inherent_owners(
        [a, b],
        [owner(
            TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
            vec![ma, mb],
        )],
    ));
}

#[test]
fn canonical_receiver_validation_checks_full_type_and_mutability() {
    let (callable, member) = method(
        "fixture::member",
        "read",
        scalar(),
        0,
        InherentMemberForm::SharedReceiver,
    );
    for receiver in [
        scalar(),
        TrustedTypeExpr::Reference {
            mutability: ReferenceMutability::Mutable,
            referent: Box::new(scalar()),
        },
        TrustedTypeExpr::Reference {
            mutability: ReferenceMutability::Shared,
            referent: Box::new(TrustedTypeExpr::Primitive(PrimitiveType::Str)),
        },
    ] {
        let invalid = change_callable(&callable, |kind| {
            let TrustedSignatureKind::Callable { parameters, .. } = kind else {
                unreachable!()
            };
            parameters[0].ty = receiver;
        });
        let trusted = TrustedSignatureEnvironment::with_inherent_owners(
            [invalid],
            [owner(
                TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
                vec![member.clone()],
            )],
        )
        .unwrap();
        assert!(matches!(
            canonicalize(&trusted),
            Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::InvalidInherentMember { .. }
            ))
        ));
    }
}

#[test]
fn applied_owner_patterns_use_existing_type_position_and_arity_validation() {
    let root = TrustedTypeExpr::Nominal {
        head: id("fixture::Reader"),
        arguments: Box::new([]),
    };
    for pattern in [
        applied(IntrinsicNominal::Box, applied(IntrinsicNominal::Own, root)),
        applied(
            IntrinsicNominal::Box,
            TrustedTypeExpr::Reference {
                mutability: ReferenceMutability::Shared,
                referent: Box::new(scalar()),
            },
        ),
        applied(
            IntrinsicNominal::Box,
            TrustedTypeExpr::Nominal {
                head: id("fixture::Unknown"),
                arguments: Box::new([]),
            },
        ),
        TrustedTypeExpr::Intrinsic {
            head: IntrinsicNominal::Box,
            arguments: Box::new([]),
        },
    ] {
        let (callable, member) = method(
            "fixture::member",
            "new",
            pattern,
            0,
            InherentMemberForm::Associated,
        );
        let trusted = TrustedSignatureEnvironment::with_inherent_owners(
            [nominal("fixture::Reader", 0), callable],
            [owner(
                TrustedInherentOwnerHead::Intrinsic(IntrinsicNominal::Box),
                vec![member],
            )],
        )
        .unwrap();
        assert!(canonicalize(&trusted).is_err());
    }
}

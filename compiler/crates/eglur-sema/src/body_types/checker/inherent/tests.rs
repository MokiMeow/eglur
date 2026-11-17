use super::*;
use crate::body_types::checker::test_support::{root, with_checker, with_checker_inputs};
use crate::{
    ExternalDeclaration, ExternalDefId, ModuleAddress, TrustedCallableParameter,
    TrustedDeclarationSignature, TrustedInherentMember, TrustedInherentOwner,
    TrustedInherentOwnerHead, TrustedSignatureEnvironment, TrustedSignatureKind, TrustedTypeExpr,
};

#[test]
fn poisoned_hidden_member_never_becomes_a_call_or_fn_instance() {
    let owner = ExternalDefId::new("fixture::Owner").unwrap();
    let method = ExternalDefId::new("fixture::read").unwrap();
    let module = ModuleAddress::new(["fixture".into()]).unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [ExternalDeclaration::new(
            owner.clone(),
            module,
            NameKey::new("Owner", false).unwrap(),
            DeclarationKind::OpaqueType,
        )
        .unwrap()],
        [owner.clone()],
    )
    .unwrap();
    let owner_pattern = TrustedTypeExpr::Nominal {
        head: owner.clone(),
        arguments: Box::new([]),
    };
    let trusted = TrustedSignatureEnvironment::with_inherent_owners(
        [
            TrustedDeclarationSignature::new(
                owner.clone(),
                TrustedSignatureKind::Nominal {
                    generic_parameters: Box::new([]),
                },
            ),
            TrustedDeclarationSignature::new(
                method.clone(),
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([]),
                    parameters: Box::new([TrustedCallableParameter {
                        name: Some("self".into()),
                        ty: TrustedTypeExpr::Reference {
                            mutability: ReferenceMutability::Shared,
                            referent: Box::new(owner_pattern.clone()),
                        },
                    }]),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Unit),
                    requirements: Box::new([]),
                },
            ),
        ],
        [TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Nominal(owner),
            members: Box::new([TrustedInherentMember {
                callable: method.clone(),
                name: NameKey::new("read", false).unwrap(),
                owner_pattern,
                owner_parameter_count: 0,
                form: InherentMemberForm::SharedReceiver,
                visibility: eglur_hir::Visibility::Public,
                overload: false,
            }]),
        }],
    )
    .unwrap();

    for text in [
        "fn inspect(v: &Owner) -> Unit = Owner.read(v)\n",
        "fn inspect(v: &Owner) -> Unit = ((Owner.read))(v)\n",
        "fn inspect(v: &Owner) -> Unit = v.read()\n",
        "fn inspect() -> Unit { let f: Fn(&Owner) -> Unit = Owner.read\n_ = f\nyield () }\n",
    ] {
        with_checker_inputs(text, &external, Some(&trusted), |checker| {
            let target = CanonicalTarget::External(method.clone());
            let FactState::Known(DeclarationSignature::Callable(id)) =
                checker.signatures.declaration(&target)
            else {
                panic!("known member required");
            };
            let id = *id;
            let poison = root(checker.diagnostics, 0);
            checker
                .signatures
                .poison_public_contract(target, poison.clone());
            assert!(checker.signatures.callable(id).is_some());
            checker.check_all().unwrap();
            assert!(checker.callable_instances.is_empty(), "{text}");
            assert_eq!(checker.invocations.iter().len(), 0, "{text}");
            let definition = checker
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .find(|declaration| {
                    declaration
                        .name()
                        .is_some_and(|name| name.text() == "inspect")
                })
                .unwrap()
                .id();
            let FactState::Poisoned(found) = checker.bodies.state(&definition) else {
                panic!("poisoned body required: {text}");
            };
            checker
                .diagnostics
                .select_preceding_roots(found, &poison)
                .unwrap();
        });
    }
}

#[test]
fn owner_pattern_matching_walks_shared_type_graphs_without_tree_expansion() {
    with_checker("fn pattern[T](v: T) -> Unit = ()\n", |checker| {
        let definition = checker
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|name| name.text() == "pattern")
            })
            .unwrap()
            .id();
        let FactState::Known(DeclarationSignature::Callable(id)) =
            checker.signatures.source_declaration(definition)
        else {
            panic!("generic signature required");
        };
        let parameter = checker
            .signatures
            .callable(*id)
            .unwrap()
            .generic_parameters()[0]
            .clone();
        let mut pattern = checker
            .signatures
            .types_mut()
            .generic_parameter(parameter.clone())
            .unwrap();
        let mut subject = checker.primitive(PrimitiveType::Bool).unwrap();
        for _ in 0..64 {
            pattern = checker
                .signatures
                .types_mut()
                .tuple([pattern, pattern])
                .unwrap();
            subject = checker
                .signatures
                .types_mut()
                .tuple([subject, subject])
                .unwrap();
        }
        let mut replacements = BTreeMap::new();
        assert!(
            checker
                .match_inherent_owner_pattern(
                    pattern,
                    subject,
                    std::slice::from_ref(&parameter),
                    &mut replacements
                )
                .unwrap()
        );
        assert_eq!(
            replacements[&parameter],
            checker.primitive(PrimitiveType::Bool).unwrap()
        );
        let integer = checker.primitive(PrimitiveType::I64).unwrap();
        let inconsistent = checker
            .signatures
            .types_mut()
            .tuple([subject, integer])
            .unwrap();
        let repeated = checker
            .signatures
            .types_mut()
            .tuple([pattern, pattern])
            .unwrap();
        assert!(
            !checker
                .match_inherent_owner_pattern(
                    repeated,
                    inconsistent,
                    &[parameter],
                    &mut BTreeMap::new()
                )
                .unwrap()
        );
    });
}

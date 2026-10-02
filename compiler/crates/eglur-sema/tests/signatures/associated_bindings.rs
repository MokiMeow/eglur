use super::*;

fn source_binding(rhs: &str) -> Analysis {
    analyze(
        &format!(
            "type Identity[T] = T\n\
             type Erased[T] = Unit\n\
             type Borrow[T] = &T\n\
             trait Linked {{ type Item }}\n\
             struct Node {{}}\n\
             impl Linked for Node {{ type Item = {rhs} }}\n"
        ),
        &CapabilitySet::signatures(),
        Some(&empty_external()),
        None,
    )
}

#[test]
fn source_binding_chains_preserve_finite_and_cycle_affected_facts() {
    for size in [1, 8, 32, 128] {
        for cycle in [false, true] {
            let mut text = String::from("trait Linked {\n");
            for node in 0..size {
                text.push_str(&format!("type A{node}\n"));
            }
            text.push_str("}\nstruct Node {}\nimpl Linked for Node {\n");
            for node in 0..size {
                let rhs = if node + 1 < size {
                    format!("Self::A{}", node + 1)
                } else if cycle {
                    format!("Self::A{node}")
                } else {
                    String::from("i64")
                };
                text.push_str(&format!("type A{node} = {rhs}\n"));
            }
            text.push_str("}\n");
            let analysis = analyze(
                &text,
                &CapabilitySet::signatures(),
                Some(&empty_external()),
                None,
            );
            assert_eq!(
                diagnostic_codes(&analysis.diagnostics),
                if cycle {
                    vec!["ASSOCIATED_TYPE_CYCLE"]
                } else {
                    vec![]
                },
                "size={size}, cycle={cycle}"
            );
            let signatures = analysis.signatures.unwrap();
            for node in 0..size {
                let binding = source_def(
                    &analysis.declarations,
                    &format!("A{node}"),
                    DeclarationKind::AssociatedTypeBinding,
                );
                let fact = signatures.source_declaration(binding);
                assert!(if cycle {
                    matches!(fact, FactState::Poisoned(_))
                } else {
                    matches!(fact, FactState::Known(_))
                });
            }
        }
    }
}

#[test]
fn multiple_cycles_blame_the_first_written_cyclic_binding_not_a_dependent() {
    let text = "trait Linked {\n type A\n type B\n type C\n type D\n type Dependent\n type Free\n }\n\
                struct Node {}\n impl Linked for Node {\n\
                type Dependent = Self::A\n\
                type D = Self::C\n\
                type B = Self::A\n\
                type C = Self::D\n\
                type A = Self::B\n\
                type Free = i64\n }\n";
    let prepared = prepare("main.eglur", text);
    let expected_span = prepared
        .hir
        .items()
        .find_map(|(_, item)| {
            let eglur_hir::ItemKind::AssociatedTypeBinding(binding) = item.kind() else {
                return None;
            };
            (binding.name.text() == Some("D")).then(|| {
                prepared
                    .hir
                    .origin(prepared.hir.type_ref(binding.value).origin())
                    .source()
            })
        })
        .unwrap();
    let analysis = analyze(
        text,
        &CapabilitySet::signatures(),
        Some(&empty_external()),
        None,
    );
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        ["ASSOCIATED_TYPE_CYCLE"]
    );
    assert_eq!(
        analysis.diagnostics.diagnostics()[0]
            .primary_span()
            .as_source()
            .unwrap()
            .file_span(),
        expected_span
    );
    let signatures = analysis.signatures.unwrap();
    let cycle = source_def(
        &analysis.declarations,
        "A",
        DeclarationKind::AssociatedTypeBinding,
    );
    let FactState::Poisoned(expected_poison) = signatures.source_declaration(cycle) else {
        panic!("cycle binding has no poison");
    };
    for name in ["A", "B", "C", "D", "Dependent"] {
        let binding = source_def(
            &analysis.declarations,
            name,
            DeclarationKind::AssociatedTypeBinding,
        );
        let FactState::Poisoned(poison) = signatures.source_declaration(binding) else {
            panic!("affected binding {name} has no poison");
        };
        poison
            .merge(expected_poison)
            .expect("cycle dependents retain the same diagnostic store");
    }
    let free = source_def(
        &analysis.declarations,
        "Free",
        DeclarationKind::AssociatedTypeBinding,
    );
    assert!(matches!(
        signatures.source_declaration(free),
        FactState::Known(_)
    ));
}

#[test]
fn associated_cycles_precede_structural_checks_through_nested_types_and_aliases() {
    for rhs in [
        "(&i64, Self::Item)",
        "(&mut i64, Self::Item)",
        "Opt[&Self::Item]",
        "(Own[i64], Self::Item)",
        "Opt[Own[Self::Item]]",
        "Fn(&Self::Item) -> i64",
        "Fn(i64) -> &Self::Item",
        "Identity[(&i64, Self::Item)]",
        "Borrow[Self::Item]",
        "(Erased[&i64], Self::Item)",
        "(Erased[Own[i64]], Self::Item)",
    ] {
        let analysis = source_binding(rhs);
        assert_eq!(
            diagnostic_codes(&analysis.diagnostics),
            ["ASSOCIATED_TYPE_CYCLE"],
            "{rhs}"
        );
        let binding = source_def(
            &analysis.declarations,
            "Item",
            DeclarationKind::AssociatedTypeBinding,
        );
        assert!(matches!(
            analysis.signatures.unwrap().source_declaration(binding),
            FactState::Poisoned(_)
        ));
    }
}

#[test]
fn mutual_cycles_poison_dependents_without_rejecting_independent_bindings() {
    let analysis = analyze(
        "trait Linked {\n type First\n type Second\n type Dependent\n type Independent\n }\n\
         struct Node {}\n\
         impl Linked for Node {\n\
           type Dependent = Opt[Self::First]\n\
           type First = (&i64, Self::Second)\n\
           type Second = (Own[i64], Self::First)\n\
           type Independent = i64\n\
         }\n",
        &CapabilitySet::signatures(),
        Some(&empty_external()),
        None,
    );
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        ["ASSOCIATED_TYPE_CYCLE"]
    );
    let signatures = analysis.signatures.unwrap();
    for name in ["First", "Second", "Dependent"] {
        let binding = source_def(
            &analysis.declarations,
            name,
            DeclarationKind::AssociatedTypeBinding,
        );
        assert!(
            matches!(
                signatures.source_declaration(binding),
                FactState::Poisoned(_)
            ),
            "{name}"
        );
    }
    let independent = source_def(
        &analysis.declarations,
        "Independent",
        DeclarationKind::AssociatedTypeBinding,
    );
    assert!(matches!(
        signatures.source_declaration(independent),
        FactState::Known(_)
    ));
}

#[test]
fn finite_binding_checks_preserve_erased_arguments_and_ownership_priority() {
    for (rhs, expected) in [
        ("&i64", "REFERENCE_TYPE_POSITION_FORBIDDEN"),
        ("Opt[&i64]", "REFERENCE_TYPE_POSITION_FORBIDDEN"),
        ("Fn(&i64) -> i64", "REFERENCE_TYPE_POSITION_FORBIDDEN"),
        ("Identity[&i64]", "REFERENCE_TYPE_POSITION_FORBIDDEN"),
        ("Erased[&Self::Item]", "REFERENCE_TYPE_POSITION_FORBIDDEN"),
        ("(Opt[&i64], Own[i64])", "OWNERSHIP_TYPE_POSITION_FORBIDDEN"),
        ("(Own[i64], Opt[&i64])", "OWNERSHIP_TYPE_POSITION_FORBIDDEN"),
        (
            "(Erased[&i64], Erased[Own[i64]])",
            "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
        ),
        (
            "(Erased[Own[i64]], &i64)",
            "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
        ),
    ] {
        let analysis = source_binding(rhs);
        assert_eq!(diagnostic_codes(&analysis.diagnostics), [expected], "{rhs}");
        let marker = if expected == "OWNERSHIP_TYPE_POSITION_FORBIDDEN" {
            "Own"
        } else {
            "reference"
        };
        for fragment in [marker, "type position", "forbidden"] {
            assert!(
                analysis.diagnostics.diagnostics()[0]
                    .message()
                    .contains(fragment),
                "{rhs}: missing {fragment}"
            );
        }
    }
    for rhs in ["i64", "Opt[i64]", "Erased[Self::Item]", "Fn(i64) -> i64"] {
        let analysis = source_binding(rhs);
        assert!(diagnostic_codes(&analysis.diagnostics).is_empty(), "{rhs}");
    }
}

#[test]
fn binding_completeness_precedes_rhs_errors() {
    for bindings in [
        "type First = (&i64, Self::First)",
        "type First = Missing\n type First = i64\n type Second = i64",
        "type First = Missing\n type Second = i64\n type Extra = i64",
    ] {
        let source = format!(
            "trait Linked {{\n type First\n type Second\n }}\n struct Node {{}}\n impl Linked for Node {{\n {bindings}\n }}\n"
        );
        let analysis = analyze(
            &source,
            &CapabilitySet::signatures(),
            Some(&empty_external()),
            None,
        );
        assert_eq!(
            diagnostic_codes(&analysis.diagnostics),
            ["TRAIT_IMPL_ASSOC_TYPE_MISMATCH"],
            "{source}"
        );
    }
}

#[test]
fn binding_name_and_projection_errors_still_precede_cycle_classification() {
    for (rhs, expected) in [
        ("(&i64, Self::Item, Missing)", "NAME_NOT_FOUND"),
        (
            "(&i64, Self::Item::Nested)",
            "ASSOCIATED_TYPE_PROJECTION_INVALID",
        ),
        (
            "(&i64, Identity[Self::Item, i64])",
            "TYPE_ARG_ARITY_MISMATCH",
        ),
    ] {
        let analysis = source_binding(rhs);
        assert_eq!(diagnostic_codes(&analysis.diagnostics), [expected], "{rhs}");
    }
}

fn trusted_bindings(
    values: impl FnOnce(TrustedTypeExpr) -> Vec<TrustedTypeExpr>,
    required: usize,
) -> Result<Analysis, SignatureResolutionError> {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("linked")]).unwrap();
    let linked = ExternalDefId::new("std::linked::Linked").unwrap();
    let item = ExternalDefId::new("std::linked::Linked::Item").unwrap();
    let node = ExternalDefId::new("std::linked::Node").unwrap();
    let node_type = TrustedTypeExpr::Nominal {
        head: node.clone(),
        arguments: Box::new([]),
    };
    let projection = TrustedTypeExpr::AssociatedProjection {
        base: Box::new(node_type.clone()),
        trait_declaration: linked.clone(),
        trait_arguments: Box::new([]),
        associated: item.clone(),
    };
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            ExternalDeclaration::new(
                linked.clone(),
                module.clone(),
                NameKey::new("Linked", false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
            ExternalDeclaration::new(
                node.clone(),
                module,
                NameKey::new("Node", false).unwrap(),
                DeclarationKind::Struct,
            )
            .unwrap(),
        ],
        [linked.clone(), node.clone()],
    )
    .unwrap();
    let names = [
        item.clone(),
        ExternalDefId::new("std::linked::Linked::Other").unwrap(),
    ];
    let values = values(projection);
    let trusted = TrustedSignatureEnvironment::new([
        TrustedDeclarationSignature::new(
            linked.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: names
                    .iter()
                    .take(required)
                    .enumerate()
                    .map(|(index, id)| TrustedAssociatedType {
                        id: id.clone(),
                        name: if index == 0 {
                            "Item".into()
                        } else {
                            "Other".into()
                        },
                    })
                    .collect(),
            },
        ),
        TrustedDeclarationSignature::new(
            node,
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            ExternalDefId::new("std::linked::impl").unwrap(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: linked,
                    arguments: Box::new([]),
                },
                target: node_type,
                associated_bindings: values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| TrustedAssociatedBinding {
                        declaration: ExternalDefId::new(format!(
                            "std::linked::impl::binding{index}"
                        ))
                        .unwrap(),
                        associated: names[index].clone(),
                        value,
                    })
                    .collect(),
            },
        ),
    ])
    .unwrap();
    try_analyze(
        "fn local() -> Unit = ()\n",
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
    )
}

#[test]
fn trusted_cycles_precede_reference_and_ownership_checks() {
    for wrapper in 0..5 {
        let result = trusted_bindings(
            |projection| {
                let reference = TrustedTypeExpr::Reference {
                    mutability: ReferenceMutability::Shared,
                    referent: Box::new(projection.clone()),
                };
                vec![match wrapper {
                    0 => reference,
                    1 => TrustedTypeExpr::Intrinsic {
                        head: IntrinsicNominal::Opt,
                        arguments: Box::new([reference]),
                    },
                    2 => TrustedTypeExpr::Intrinsic {
                        head: IntrinsicNominal::Own,
                        arguments: Box::new([projection]),
                    },
                    3 => TrustedTypeExpr::Callable {
                        parameters: Box::new([reference]),
                        result: Box::new(TrustedTypeExpr::Primitive(
                            eglur_sema::PrimitiveType::I64,
                        )),
                    },
                    _ => TrustedTypeExpr::Tuple(Box::new([reference, projection])),
                }]
            },
            1,
        );
        assert!(
            matches!(
                result,
                Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        reason: "associated type binding cycle",
                        ..
                    }
                ))
            ),
            "wrapper {wrapper}"
        );
    }
}

#[test]
fn trusted_completeness_precedes_supplied_rhs_validation() {
    let result = trusted_bindings(|_| vec![TrustedTypeExpr::GenericParameter(99)], 2);
    assert!(matches!(
        result,
        Err(SignatureResolutionError::Trusted(
            TrustedSignatureError::UnknownAssociatedType(_)
        ))
    ));
}

#[test]
fn trusted_mutual_cycles_keep_all_structural_dependency_edges() {
    let result = trusted_bindings(
        |first| {
            let mut second = first.clone();
            let TrustedTypeExpr::AssociatedProjection { associated, .. } = &mut second else {
                unreachable!()
            };
            *associated = ExternalDefId::new("std::linked::Linked::Other").unwrap();
            vec![
                TrustedTypeExpr::Reference {
                    mutability: ReferenceMutability::Shared,
                    referent: Box::new(second),
                },
                TrustedTypeExpr::Intrinsic {
                    head: IntrinsicNominal::Own,
                    arguments: Box::new([first]),
                },
            ]
        },
        2,
    );
    assert!(matches!(
        result,
        Err(SignatureResolutionError::Trusted(
            TrustedSignatureError::InvalidTypePosition {
                reason: "associated type binding cycle",
                ..
            }
        ))
    ));
}

#[test]
fn finite_trusted_bindings_keep_structural_rejections_and_valid_types() {
    let integer = TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64);
    let reference = TrustedTypeExpr::Reference {
        mutability: ReferenceMutability::Shared,
        referent: Box::new(integer.clone()),
    };
    let owned = TrustedTypeExpr::Intrinsic {
        head: IntrinsicNominal::Own,
        arguments: Box::new([integer.clone()]),
    };
    for (value, category) in [
        (reference.clone(), "reference"),
        (
            TrustedTypeExpr::Intrinsic {
                head: IntrinsicNominal::Opt,
                arguments: Box::new([reference.clone()]),
            },
            "reference",
        ),
        (
            TrustedTypeExpr::Tuple(Box::new([reference.clone(), owned.clone()])),
            "ownership",
        ),
        (
            TrustedTypeExpr::Tuple(Box::new([owned, reference])),
            "ownership",
        ),
    ] {
        let result = trusted_bindings(|_| vec![value], 1);
        assert!(matches!(result, Err(SignatureResolutionError::Trusted(
            TrustedSignatureError::InvalidTypePosition { reason, .. }
        )) if reason.contains(category)));
    }
    let analysis = trusted_bindings(|_| vec![integer], 1).unwrap();
    assert!(diagnostic_codes(&analysis.diagnostics).is_empty());
}

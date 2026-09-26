use super::*;
use eglur_sema::TypeOccurrenceId;

const TYPE_FORMS: &[(&str, &str)] = &[
    ("& ", ""),
    ("&mut ", ""),
    ("Opt[", "]"),
    ("Box[", "]"),
    ("Boxed[", "]"),
    ("(", ")"),
    ("(", ", i64)"),
    ("Fn(", ") -> i64"),
    ("Fn() -> ", ""),
];

fn nested(prefix: &str, leaf: &str, suffix: &str, depth: usize) -> String {
    format!("{}{leaf}{}", prefix.repeat(depth), suffix.repeat(depth))
}

#[test]
fn nested_intrinsic_signatures_resolve_on_the_default_stack() {
    let depth = 120;
    let ty = nested("Opt[", "i64", "]", depth);
    let source = format!("fn f(input: {ty}) -> Unit = ()\n");
    let prepared = prepare("main.eglur", &source);
    let external = empty_external();
    let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
    assert!(analysis.diagnostics.diagnostics().is_empty());
    assert!(
        analysis
            .demands
            .iter()
            .all(|demand| { demand.disposition() != DemandDisposition::Missing })
    );

    let signatures = analysis.signatures.unwrap();
    assert_eq!(signatures.occurrence_fact_count(), depth + 2);
    for (type_ref, _) in prepared.hir.type_refs() {
        assert!(matches!(
            signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref)),
            FactState::Known(_)
        ));
    }
    let function = source_def(&analysis.declarations, "f", DeclarationKind::Function);
    assert!(matches!(
        signatures.source_declaration(function),
        FactState::Known(DeclarationSignature::Callable(_))
    ));
}

#[test]
fn source_type_families_resolve_at_the_parser_budget() {
    for &(prefix, suffix) in TYPE_FORMS {
        let ty = nested(prefix, "i64", suffix, 255);
        let bang = if prefix == "&mut " { "!" } else { "" };
        let source = format!("struct Boxed[T] {{}}\nfn f{bang}(input: {ty}) -> Unit = ()\n");
        let prepared = prepare("main.eglur", &source);
        let external = empty_external();
        let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
        assert!(
            analysis.diagnostics.diagnostics().is_empty(),
            "{prefix}: {:?}",
            diagnostic_codes(&analysis.diagnostics)
        );

        let signatures = analysis.signatures.unwrap();
        for (type_ref, _) in prepared.hir.type_refs() {
            assert!(
                matches!(
                    signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref)),
                    FactState::Known(_)
                ),
                "{prefix}: {type_ref:?}"
            );
        }
        let function = source_def(&analysis.declarations, "f", DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Known(DeclarationSignature::Callable(_))
        ));
    }
}

#[test]
fn deep_type_failures_keep_later_siblings_and_return_facts() {
    for &(prefix, suffix) in TYPE_FORMS {
        let ty = nested(prefix, "Missing", suffix, 255);
        let bang = if prefix == "&mut " { "!" } else { "" };
        let source = format!("struct Boxed[T] {{}}\nfn f{bang}(input: {ty}) -> Unit = ()\n");
        let prepared = prepare("main.eglur", &source);
        let external = empty_external();
        let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
        assert_eq!(diagnostic_codes(&analysis.diagnostics), ["NAME_NOT_FOUND"]);

        let signatures = analysis.signatures.unwrap();
        for (type_ref, occurrence) in prepared.hir.type_refs() {
            let fact = signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref));
            let independent = matches!(
                occurrence.kind(),
                eglur_hir::TypeRefKind::Path { path, arguments }
                    if arguments.is_empty()
                        && path.segments().len() == 1
                        && matches!(path.segments()[0].text(), Some("i64" | "Unit"))
            );
            assert!(
                if independent {
                    matches!(fact, FactState::Known(_))
                } else {
                    matches!(fact, FactState::Poisoned(_))
                },
                "{prefix}: {type_ref:?}: {fact:?}"
            );
        }
    }
}

#[test]
fn rejected_heads_do_not_schedule_their_type_arguments() {
    let deep = nested("Opt[", "Missing", "]", 240);
    for (ty, code) in [
        (format!("Opt[{deep}, Missing]"), "TYPE_ARG_ARITY_MISMATCH"),
        (format!("Boxed[{deep}, Missing]"), "TYPE_ARG_ARITY_MISMATCH"),
        (format!("Alias[{deep}, Missing]"), "TYPE_ARG_ARITY_MISMATCH"),
        (format!("Union[{deep}, Missing]"), "UNION_TYPE_UNSUPPORTED"),
        (format!("*const {deep}"), "RAW_POINTER_TYPE_UNSUPPORTED"),
    ] {
        let source =
            format!("struct Boxed[T] {{}}\ntype Alias[T] = T\nfn f(input: {ty}) -> Unit = ()\n");
        let prepared = prepare("main.eglur", &source);
        let external = empty_external();
        let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
        assert_eq!(diagnostic_codes(&analysis.diagnostics), [code]);

        let signatures = analysis.signatures.unwrap();
        for (type_ref, occurrence) in prepared.hir.type_refs() {
            if matches!(occurrence.kind(), eglur_hir::TypeRefKind::Path { path, .. }
                if path.segments()[0].text() == Some("Missing"))
            {
                assert!(matches!(
                    signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref)),
                    FactState::Unperformed
                ));
            }
        }
    }
}

#[test]
fn deep_extern_pointers_keep_nested_facts_before_rejecting_the_abi_type() {
    for prefix in ["*const ", "*mut "] {
        let ty = nested(prefix, "i64", "", 255);
        let source = format!("extern \"C\" fn f(input: {ty}) -> Unit\n");
        let prepared = prepare("main.eglur", &source);
        let external = empty_external();
        let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
        assert_eq!(
            diagnostic_codes(&analysis.diagnostics),
            ["EXTERN_SIGNATURE_UNSUPPORTED"]
        );
        let signatures = analysis.signatures.unwrap();
        let poisoned = prepared
            .hir
            .type_refs()
            .filter(|(type_ref, _)| {
                matches!(
                    signatures.occurrence(TypeOccurrenceId::new(prepared.file, *type_ref)),
                    FactState::Poisoned(_)
                )
            })
            .count();
        assert_eq!(poisoned, 1);
        assert_eq!(signatures.occurrence_fact_count(), 257);
    }
}

#[test]
fn nested_trait_arguments_share_the_type_worklist() {
    let external = empty_external();
    let ty = nested("Opt[", "i64", "]", 250);
    let source = format!("trait Mark[T] {{}}\nfn f[T: Mark[{ty}]](input: T) -> Unit = ()\n");
    let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
    assert!(analysis.diagnostics.diagnostics().is_empty());
    let function = source_def(&analysis.declarations, "f", DeclarationKind::Function);
    assert!(matches!(
        analysis.signatures.unwrap().source_declaration(function),
        FactState::Known(DeclarationSignature::Callable(_))
    ));

    let ty = nested("dyn Mark[", "i64", "]", 120);
    let source = format!("trait Mark[T] {{}}\nfn f(input: {ty}) -> Unit = ()\n");
    let prepared = prepare("main.eglur", &source);
    let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        vec!["DYN_TRAIT_UNSUPPORTED"; 120]
    );
    assert_eq!(
        analysis.signatures.unwrap().occurrence_fact_count(),
        prepared.hir.type_refs().len()
    );
}

fn alias_chain(depth: usize, cycle: bool) -> String {
    let mut source = String::new();
    for index in 0..depth {
        source.push_str(&format!("type Alias{index}[T] = Alias{}[T]\n", index + 1));
    }
    let leaf = if cycle { "Alias0[T]" } else { "T" };
    source.push_str(&format!("type Alias{depth}[T] = {leaf}\n"));
    source.push_str("fn f(input: Alias0[i64]) -> Alias0[Bool] = true\n");
    source
}

#[test]
fn alias_templates_resume_in_their_own_generic_contexts() {
    let source = alias_chain(300, false);
    let external = empty_external();
    let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
    assert!(analysis.diagnostics.diagnostics().is_empty());

    let signatures = analysis.signatures.unwrap();
    let function = source_def(&analysis.declarations, "f", DeclarationKind::Function);
    let FactState::Known(DeclarationSignature::Callable(id)) =
        signatures.source_declaration(function)
    else {
        panic!("alias consumer was not resolved")
    };
    let callable = signatures.callable(*id).unwrap();
    assert!(matches!(
        signatures
            .types()
            .kind(callable.parameters()[0].ty())
            .unwrap(),
        TypeKind::Primitive(eglur_sema::PrimitiveType::I64)
    ));
    assert!(matches!(
        signatures.types().kind(callable.result()).unwrap(),
        TypeKind::Primitive(eglur_sema::PrimitiveType::Bool)
    ));
}

#[test]
fn long_alias_cycles_poison_each_alias_and_their_consumers() {
    let source = alias_chain(300, true);
    let external = empty_external();
    let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        ["TYPE_ALIAS_CYCLE"]
    );
    let signatures = analysis.signatures.unwrap();
    for declaration in analysis
        .declarations
        .modules()
        .flat_map(|module| module.declarations())
    {
        if matches!(
            declaration.kind(),
            DeclarationKind::TypeAlias | DeclarationKind::Function
        ) {
            assert!(matches!(
                signatures.source_declaration(declaration.id()),
                FactState::Poisoned(_)
            ));
        }
    }
}

#[test]
fn partial_type_sequences_keep_poison_distinct_from_missing_support() {
    for (ty, expected, poisoned) in [
        ("Fn(Missing) -> Bool", &[][..], false),
        (
            "Fn(Missing, Opt[i64, i64]) -> Bool",
            &["TYPE_ARG_ARITY_MISMATCH"][..],
            true,
        ),
        (
            "Fn(Opt[i64, i64]) -> Missing",
            &["TYPE_ARG_ARITY_MISMATCH"][..],
            true,
        ),
        ("dyn Mark[Missing]", &["DYN_TRAIT_UNSUPPORTED"][..], false),
        (
            "(dyn Mark[Missing], Opt[i64, i64], Bool)",
            &["DYN_TRAIT_UNSUPPORTED", "TYPE_ARG_ARITY_MISMATCH"][..],
            true,
        ),
    ] {
        let source = format!("trait Mark[T] {{}}\nfn f(input: {ty}) -> Unit = ()\n");
        let prepared = prepare("main.eglur", &source);
        let analysis = analyze(&source, &CapabilitySet::signatures(), None, None);
        assert_eq!(diagnostic_codes(&analysis.diagnostics), expected, "{ty}");
        let signatures = analysis.signatures.unwrap();
        let function = source_def(&analysis.declarations, "f", DeclarationKind::Function);
        let fact = signatures.source_declaration(function);
        assert!(
            if poisoned {
                matches!(fact, FactState::Poisoned(_))
            } else {
                matches!(fact, FactState::Unperformed)
            },
            "{ty}: {fact:?}"
        );

        for (type_ref, occurrence) in prepared.hir.type_refs() {
            let eglur_hir::TypeRefKind::Path { path, .. } = occurrence.kind() else {
                continue;
            };
            let fact = signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref));
            match path.segments()[0].text() {
                Some("Missing") => assert!(matches!(fact, FactState::Unperformed)),
                Some("Bool" | "Unit") => assert!(matches!(fact, FactState::Known(_))),
                _ => {}
            }
        }
    }
}

#[test]
fn dtype_bool_is_recognized_only_in_constructor_position() {
    let source = concat!(
        "fn ordinary(input: bool) -> Unit = ()\n",
        "fn constructor() -> Unit { let value = zeros[bool](2)\nyield () }\n",
    );
    let prepared = prepare("main.eglur", source);
    let external = empty_external();
    let analysis = analyze(source, &CapabilitySet::signatures(), Some(&external), None);
    assert_eq!(diagnostic_codes(&analysis.diagnostics), ["NAME_NOT_FOUND"]);
    let signatures = analysis.signatures.unwrap();
    let bool_facts = prepared
        .hir
        .type_refs()
        .filter_map(|(type_ref, occurrence)| {
            let eglur_hir::TypeRefKind::Path { path, .. } = occurrence.kind() else {
                return None;
            };
            (path.segments()[0].text() == Some("bool"))
                .then(|| signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref)))
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        bool_facts.as_slice(),
        [FactState::Poisoned(_), FactState::Known(_)]
    ));
}

#[test]
fn local_annotation_entrypoints_share_the_bounded_type_resolver() {
    let ty = nested("Opt[", "i64", "]", 240);
    let source = format!("fn f() -> Unit {{ let local: {ty} = ()\nyield () }}\n");
    let prepared = prepare("main.eglur", &source);
    let external = empty_external();
    let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
    assert!(analysis.diagnostics.diagnostics().is_empty());
    let signatures = analysis.signatures.unwrap();
    for (type_ref, _) in prepared.hir.type_refs() {
        assert!(matches!(
            signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref)),
            FactState::Known(_)
        ));
    }
}

#[test]
fn deep_alias_templates_preserve_canonical_instantiation_identity() {
    let template = nested("Opt[", "T", "]", 255);
    let source =
        format!("type Nested[T] = {template}\nfn f(input: Nested[i64]) -> Nested[i64] = input\n");
    let external = empty_external();
    let analysis = analyze(&source, &CapabilitySet::signatures(), Some(&external), None);
    assert!(analysis.diagnostics.diagnostics().is_empty());
    let signatures = analysis.signatures.unwrap();
    let function = source_def(&analysis.declarations, "f", DeclarationKind::Function);
    let FactState::Known(DeclarationSignature::Callable(id)) =
        signatures.source_declaration(function)
    else {
        panic!("alias application was not resolved")
    };
    let callable = signatures.callable(*id).unwrap();
    let mut ty = callable.parameters()[0].ty();
    assert_eq!(ty, callable.result());
    for _ in 0..255 {
        let TypeKind::Nominal { head, substitution } = signatures.types().kind(ty).unwrap() else {
            panic!("expected Opt layer")
        };
        assert_eq!(*head, NominalHead::Intrinsic(IntrinsicNominal::Opt));
        let [child] = signatures.types().substitution(*substitution).unwrap() else {
            panic!("expected one argument")
        };
        ty = *child;
    }
    assert!(matches!(
        signatures.types().kind(ty).unwrap(),
        TypeKind::Primitive(eglur_sema::PrimitiveType::I64)
    ));
}

#[test]
fn deep_trusted_nominals_preserve_missing_signature_boundaries() {
    let module = ModuleAddress::new([Box::<str>::from("remote")]).unwrap();
    let remote = ExternalDefId::new("remote::Remote").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [ExternalDeclaration::new(
            remote.clone(),
            module,
            NameKey::new("Remote", false).unwrap(),
            DeclarationKind::OpaqueType,
        )
        .unwrap()],
        [remote.clone()],
    )
    .unwrap();
    let trusted = TrustedSignatureEnvironment::new([TrustedDeclarationSignature::new(
        remote,
        TrustedSignatureKind::Nominal {
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "T".into(),
                bounds: Box::new([]),
            }]),
        },
    )])
    .unwrap();

    let ty = nested("Remote[", "i64", "]", 255);
    let source = format!("fn f(input: {ty}) -> Unit = ()\n");
    let prepared = prepare("main.eglur", &source);
    for trusted in [Some(&trusted), None] {
        let analysis = analyze(
            &source,
            &CapabilitySet::signatures(),
            Some(&external),
            trusted,
        );
        assert!(analysis.diagnostics.diagnostics().is_empty());
        let signatures = analysis.signatures.unwrap();
        for (type_ref, occurrence) in prepared.hir.type_refs() {
            let fact = signatures.occurrence(TypeOccurrenceId::new(prepared.file, type_ref));
            let eglur_hir::TypeRefKind::Path { path, .. } = occurrence.kind() else {
                panic!("expected nominal or primitive")
            };
            assert!(
                if trusted.is_some() || path.segments()[0].text() == Some("Unit") {
                    matches!(fact, FactState::Known(_))
                } else {
                    matches!(fact, FactState::Unperformed)
                },
                "{type_ref:?}: {fact:?}"
            );
        }
    }
}

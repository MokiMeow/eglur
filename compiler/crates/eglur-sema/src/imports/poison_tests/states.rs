use super::*;
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticPhase, DiagnosticRoot, PrimarySpan,
    Severity,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Public,
    Private,
    Other,
    Double,
    Distinct,
    Shared,
    Wide,
    Absent,
    Inapplicable,
    Foreign,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Refusal {
    Inapplicable,
    Foreign,
}

fn root(store: &mut DiagnosticStore, index: usize) -> Poison {
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new(format!("ROOT_{index}")).unwrap(),
        Severity::Error,
        DiagnosticPhase::new("resolve").unwrap(),
        "independent imported failure",
        PrimarySpan::Unavailable,
    );
    let root = DiagnosticRoot::boundary(
        BoundaryRootKey::new(format!("root-{index}")).unwrap(),
        diagnostic,
    )
    .unwrap();
    store.record_failure(DiagnosticFailure::Root(root)).unwrap()
}

fn selected_codes(mut store: DiagnosticStore, poison: &Poison, all: &Poison) -> Vec<String> {
    let all = all.merge(poison).unwrap();
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store.select_preceding_roots(poison, &all).unwrap();
    let snapshot = store.finalize(None);
    let mut codes = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect::<Vec<_>>();
    codes.sort();
    codes
}

#[test]
fn import_lookup_preserves_partial_priority_refusal_frontiers_and_binding_policy() {
    let modes = [
        Mode::Public,
        Mode::Private,
        Mode::Other,
        Mode::Double,
        Mode::Distinct,
        Mode::Shared,
        Mode::Wide,
        Mode::Absent,
        Mode::Inapplicable,
        Mode::Foreign,
    ];
    for cross_module in [false, true] {
        let imports = "import provider::Existing as Wanted\n".repeat(3);
        let main = format!(
            "{imports}fn after(x: i64) -> i64 = x\n{}",
            if cross_module {
                ""
            } else {
                "fn probe() -> Unit = Wanted\n"
            },
        );
        let consumer = if cross_module {
            "fn probe() -> Unit = main::Wanted\n"
        } else {
            "fn unused() -> Unit = ()\n"
        };
        with_imports(
            &[
                (
                    "provider",
                    "pub struct Existing {}\npub fn existing() -> i64 = 0\npub fn later() -> i64 = 1\n".to_owned(),
                ),
                ("main", main),
                ("consumer", consumer.to_owned()),
            ],
            true,
            |collection, original, sources, support, external, _| {
                let module = collection.module_for_file(FileId::new(1)).unwrap();
                let query_module = collection
                    .module_for_file(FileId::new(if cross_module { 2 } else { 1 }))
                    .unwrap();
                let name = NameKey::new("Wanted", false).unwrap();
                let ids = original.lookup(module, Namespace::Type, &name);
                let provider = collection.module_for_file(FileId::new(0)).unwrap();
                let type_target = collection
                    .module(provider)
                    .unwrap()
                    .declarations()
                    .find(|d| d.name().is_some_and(|n| n.text() == "Existing"))
                    .unwrap()
                    .id();
                let value_target = collection
                    .module(provider)
                    .unwrap()
                    .declarations()
                    .find(|d| d.name().is_some_and(|n| n.text() == "existing"))
                    .unwrap()
                    .id();
                let later_target = collection.module(provider).unwrap().declarations().find(|d| d.name().is_some_and(|n| n.text() == "later")).unwrap().id();
                let query_source = &sources[if cross_module { 2 } else { 1 }];
                let expression = query_source
                    .hir()
                    .unwrap()
                    .bodies()
                    .flat_map(|(_, body)| body.expressions())
                    .find_map(|(id, expression)| match expression.kind() {
                        eglur_hir::ExprKind::Name(path)
                        | eglur_hir::ExprKind::VariantConstructor {
                            path,
                            payload: eglur_hir::ConstructorPayload::Absent,
                        } if path
                            .segments()
                            .last()
                            .and_then(NameKey::from_hir)
                            .is_some_and(|n| n == name) =>
                        {
                            Some(id)
                        }
                        _ => None,
                    })
                    .unwrap();
                let key = crate::ResolutionKey::Expression {
                    module: query_module,
                    expression,
                };

                let cases = modes.into_iter().flat_map(|a| {
                    modes.into_iter().flat_map(move |b| {
                        modes.into_iter().map(move |c| [a, b, c])
                    })
                }).flat_map(|inputs| {
                    [0, 1].into_iter().map(move |stage| (inputs, stage))
                });
                for (inputs, stage) in cases {
                    let mut diagnostics = DiagnosticStore::new();
                    let roots = (0..12)
                        .map(|index| root(&mut diagnostics, index))
                        .collect::<Vec<_>>();
                    let mut pending = PoisonAccumulator::new();
                    for root in &roots {
                        pending.include(root).unwrap();
                    }
                    let all = pending.finish().unwrap();
                    let mut foreign_store = DiagnosticStore::new();
                    let foreign = root(&mut foreign_store, 999);
                    let mut facts = FactTable::new();
                    let mut expected_roots = BTreeSet::new();
                    let mut lineage = None;
                    let mut refusal = None;
                    let mut work = Work::default();
                    let mut unavailable = false;
                    let mut visible = 0;
                    let mut private = false;
                    for (index, (&id, mode)) in ids.iter().zip(inputs).enumerate() {
                        let poison = match mode {
                            Mode::Distinct => Some(roots[index].clone()),
                            Mode::Shared => Some(roots[0].clone()),
                            Mode::Wide => Some(all.clone()),
                            Mode::Foreign => Some(foreign.clone()),
                            _ => None,
                        };
                        let fact = if let Some(poison) = poison {
                            SemanticFact::Poisoned(poison)
                        } else if mode == Mode::Absent {
                            unavailable = true;
                            continue;
                        } else if mode == Mode::Inapplicable {
                            SemanticFact::NotApplicable
                        } else {
                            let local = if mode == Mode::Other {
                                NameKey::new("Other", false).unwrap()
                            } else {
                                name.clone()
                            };
                            let mut bindings = Vec::new();
                            for occurrence in 0..if mode == Mode::Double { 2 } else { 1 } {
                                for (namespace, target) in [
                                    (Namespace::Type, type_target),
                                    (Namespace::Value, value_target),
                                ] {
                                    let target = if namespace == Namespace::Value && occurrence == 1 { later_target } else { target };
                                    bindings.push(ImportedBinding {
                                        local_name: local.clone(),
                                        namespace,
                                        targets: Box::new([CanonicalTarget::Source(
                                            target,
                                        )]),
                                        public: mode != Mode::Private,
                                    });
                                }
                            }
                            SemanticFact::Known(bindings.into_boxed_slice())
                        };
                        facts.insert(id, fact).unwrap();

                        if refusal.is_some() {
                            continue;
                        }
                        if mode == Mode::Inapplicable {
                            refusal = Some(Refusal::Inapplicable);
                        } else if matches!(
                            mode,
                            Mode::Distinct | Mode::Shared | Mode::Wide | Mode::Foreign
                        ) {
                            work.inputs += 1;
                            let foreign_input = mode == Mode::Foreign;
                            if lineage.is_some_and(|pool| pool != foreign_input) {
                                refusal = Some(Refusal::Foreign);
                                continue;
                            }
                            lineage = Some(foreign_input);
                            match mode {
                                Mode::Distinct => {
                                    expected_roots.insert(index);
                                }
                                Mode::Shared => {
                                    expected_roots.insert(0);
                                }
                                Mode::Wide => expected_roots.extend(0..12),
                                Mode::Foreign => {
                                    expected_roots.insert(999);
                                }
                                _ => unreachable!(),
                            }
                        } else if mode != Mode::Other {
                            if cross_module && mode == Mode::Private {
                                private = true;
                            } else {
                                visible += if stage == 1 && mode == Mode::Double {
                                    2
                                } else {
                                    1
                                };
                            }
                        }
                    }
                    work.completed =
                        usize::from(refusal.is_none() && !expected_roots.is_empty());
                    let imports = ResolvedImports {
                        facts,
                        lookup: original.lookup.clone(),
                    };
                    WORK.set([Work::default(); 2]);

                    let result = if stage == 1 {
                        match lookup_source_module_type(
                            collection, &imports, collection.module(module).unwrap(), &name, cross_module,
                        ) {
                            Ok(TypeNameLookup::Poisoned(poison)) => Ok(Some(poison)),
                            Ok(TypeNameLookup::Unperformed) => {
                                assert!(unavailable);
                                Ok(None)
                            }
                            Ok(TypeNameLookup::Unique(_)) => {
                                assert!(!unavailable);
                                assert_eq!(visible, 1);
                                Ok(None)
                            }
                            Ok(TypeNameLookup::Ambiguous(targets)) => {
                                assert!(!unavailable);
                                assert!(visible > 1);
                                assert_eq!(targets.len(), visible);
                                Ok(None)
                            }
                            Ok(TypeNameLookup::VisibilityViolation) => {
                                assert!(!unavailable);
                                assert_eq!(visible, 0);
                                assert!(private);
                                Ok(None)
                            }
                            Ok(TypeNameLookup::NotFound) => {
                                assert!(!unavailable);
                                assert_eq!(visible, 0);
                                assert!(!private);
                                Ok(None)
                            }
                            Err(crate::type_namespace::TypeNameLookupError::InvalidImportFact(_)) => Err(Refusal::Inapplicable),
                            Err(crate::type_namespace::TypeNameLookupError::DiagnosticStore(DiagnosticStoreError::ForeignPoison)) => Err(Refusal::Foreign),
                            Err(error) => panic!("unexpected type lookup refusal: {error:?}"),
                        }
                    } else {
                        match resolve_names(
                            collection,
                            &imports,
                            sources,
                            support,
                            Some(external),
                            &mut diagnostics,
                        ) {
                            Ok(analysis) => {
                                let PhaseProduct::Performed(names) =
                                    analysis.into_parts().0
                                else {
                                    panic!("names unavailable")
                                };
                                match names.fact(key) {
                                    FactState::Poisoned(poison) => {
                                        Ok(Some(poison.clone()))
                                    }
                                    FactState::Unperformed => {
                                        assert!(unavailable);
                                        Ok(None)
                                    }
                                    FactState::Known(ResolvedName::Declarations(
                                        targets,
                                    )) => {
                                        assert!(!unavailable);
                                        assert_eq!(visible, 1);
                                        assert_eq!(
                                            targets.as_ref(),
                                            [CanonicalTarget::Source(value_target)]
                                        );
                                        Ok(None)
                                    }
                                    _ => panic!("unexpected name lookup fact"),
                                }
                            }
                            Err(crate::NameResolutionError::InvalidImportFact) => {
                                Err(Refusal::Inapplicable)
                            }
                            Err(crate::NameResolutionError::DiagnosticStore(
                                DiagnosticStoreError::ForeignPoison,
                            )) => Err(Refusal::Foreign),
                            Err(error) => {
                                panic!("unexpected name lookup refusal: {error:?}")
                            }
                        }
                    };
                    let mut expected_work = [Work::default(); 2];
                    expected_work[stage] = work;
                    assert_eq!(
                        WORK.get(),
                        expected_work,
                        "{inputs:?}/{cross_module}/{stage}"
                    );
                    if let Some(refusal) = refusal {
                        assert!(matches!(result, Err(actual) if actual == refusal));
                    } else if let Some(poison) = result.unwrap() {
                        assert!(!unavailable || !expected_roots.is_empty());
                        let expected = if expected_roots.is_empty() {
                            vec![
                                if visible > 1 {
                                    "NAME_AMBIGUOUS"
                                } else if private {
                                    "VISIBILITY_VIOLATION"
                                } else {
                                    "NAME_NOT_FOUND"
                                }
                                .to_owned(),
                            ]
                        } else {
                            let mut codes = expected_roots
                                .iter()
                                .map(|index| format!("ROOT_{index}"))
                                .collect::<Vec<_>>();
                            codes.sort();
                            codes
                        };
                        let actual = if lineage == Some(true) {
                            selected_codes(foreign_store, &poison, &foreign)
                        } else {
                            selected_codes(diagnostics, &poison, &all)
                        };
                        assert_eq!(
                            actual, expected,
                            "{inputs:?}/{cross_module}/{stage}"
                        );
                    } else {
                        assert!(expected_roots.is_empty());
                    }
                }
            },
        );
    }
}

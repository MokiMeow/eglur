use super::super::*;
use super::with_resolver;
use crate::owned_poison::test_support::{Work, reset, work};
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, PrimarySpan, Severity,
};

struct Support(bool);

impl CompileSupport for Support {
    fn supports(&self, capability: CompileCapability) -> bool {
        self.0 || capability != CompileCapability::Resolve(ResolveCapability::ResolveModuleValues)
    }
}

fn expected_work(poisons: usize) -> Work {
    Work {
        pairs: usize::from(poisons > 1),
        includes: if poisons > 2 { poisons - 1 } else { 0 },
        finishes: usize::from(poisons > 2),
    }
}

#[test]
fn callable_and_lambda_parameters_finish_once_without_rebinding_collisions() {
    for count in [0usize, 1, 2, 3, 4, 8, 32, 128] {
        for lambda in [false, true] {
            let parameters = vec!["x: i64"; count].join(",");
            let body = if count == 0 { "()" } else { "x" };
            let mut text = if lambda {
                format!("fn probe() -> Unit {{ let c = {{ |{parameters}| {body} }}\nyield () }}\n")
            } else {
                format!("fn probe({parameters}) -> Unit = {body}\n")
            };
            text.push_str("fn after(q: i64) -> i64 = q\n");
            with_resolver(&text, &Support(true), |resolver, source| {
                let module = resolver.collection.module_for_file(source.file()).unwrap();
                let hir = source.hir().unwrap();
                reset();
                for &item in hir.roots() {
                    resolver.visit_item(module, source, hir, item).unwrap();
                }
                let poisons = count.saturating_sub(1);
                assert_eq!(work(), expected_work(poisons), "{count}/{lambda}");
                assert!(resolver.active_scopes.is_empty());
                assert!(resolver.current_owner.is_none());
                let xs = resolver
                    .locals
                    .iter()
                    .filter(|b| b.name.text() == "x")
                    .collect::<Vec<_>>();
                assert_eq!(xs.len(), usize::from(count > 0));
                if let Some(x) = xs.first() {
                    assert_eq!(x.origins.len(), 1);
                    assert!(resolver.facts.iter().any(|(_, fact)| matches!(fact,
                        SemanticFact::Known(ResolvedName::Local(id)) if *id == x.id)));
                }
                assert_eq!(
                    resolver
                        .locals
                        .iter()
                        .filter(|b| b.name.text() == "q")
                        .count(),
                    1
                );
                let snapshot =
                    std::mem::replace(resolver.diagnostics, DiagnosticStore::new()).finalize(None);
                assert_eq!(snapshot.diagnostics().len(), poisons);
                assert!(
                    snapshot
                        .diagnostics()
                        .iter()
                        .all(|d| d.code().as_str() == "LOCAL_BINDING_REDECLARATION")
                );
            });
        }
    }
}

#[test]
fn pattern_head_siblings_keep_constructor_facts_and_finish_once() {
    for count in [0usize, 1, 2, 3, 8, 32, 128] {
        let heads = (0..count)
            .map(|i| format!("Missing{i}()"))
            .collect::<Vec<_>>();
        let tuple = if count == 0 {
            "()".to_owned()
        } else {
            format!("({},)", heads.join(","))
        };
        let fields = (0..count)
            .map(|i| format!("f{i}: Missing{i}()"))
            .collect::<Vec<_>>()
            .join(",");
        for (pattern, outer_poison, own_fact) in [
            (tuple.clone(), 0, false),
            (format!("((alias @ {tuple}))"), 0, false),
            (
                if count == 0 {
                    "_".into()
                } else {
                    heads.join("|")
                },
                0,
                false,
            ),
            (
                if count == 0 {
                    "Outer()".into()
                } else {
                    format!("Outer({},)", heads.join(","))
                },
                1,
                true,
            ),
            (format!("Outer {{ {fields} }}"), 1, true),
            (format!("S {{ {fields} }}"), 0, true),
        ] {
            let text = format!(
                "struct S {{}}\nfn probe(value: Unit) -> Unit = match value {{ {pattern} => () }}\n"
            );
            with_resolver(&text, &Support(true), |resolver, source| {
                let module = resolver.collection.module_for_file(source.file()).unwrap();
                let hir = source.hir().unwrap();
                let ItemKind::Function(function) = hir.item(hir.roots()[1]).kind() else {
                    panic!("missing function")
                };
                let body = hir.body(function.body);
                let FunctionBody::Expression(root) = function.body_form else {
                    panic!("missing body")
                };
                let ExprKind::Match(value) = body.expression(root).unwrap().kind() else {
                    panic!("missing match")
                };
                let pattern_id = value.arms[0].pattern;
                reset();
                let outcome = resolver
                    .visit_pattern_heads(module, source, body, pattern_id)
                    .unwrap();
                let poisons = count + outer_poison;
                assert_eq!(work(), expected_work(poisons), "{count}/{pattern}");
                assert_eq!(resolver.facts.iter().count(), count + usize::from(own_fact));
                if own_fact {
                    let fact = resolver.facts.state(&ResolutionKey::Pattern {
                        module,
                        pattern: pattern_id,
                    });
                    if outer_poison == 0 {
                        assert!(matches!(fact, FactState::Known(_)));
                    } else {
                        assert!(matches!(fact, FactState::Poisoned(_)));
                    }
                }
                assert_eq!(matches!(outcome, VisitOutcome::Poisoned(_)), poisons > 0);
                assert!(resolver.active_scopes.is_empty());
                let snapshot =
                    std::mem::replace(resolver.diagnostics, DiagnosticStore::new()).finalize(None);
                assert_eq!(snapshot.diagnostics().len(), poisons);
                assert!(
                    snapshot
                        .diagnostics()
                        .iter()
                        .all(|d| d.code().as_str() == "NAME_NOT_FOUND")
                );
            });
        }
    }
}

#[test]
fn unavailable_pattern_lookup_is_not_a_known_empty_result() {
    with_resolver(
        "fn probe(value: Unit) -> Unit = match value { (Missing0(), Missing1()) => () }\n",
        &Support(false),
        |resolver, source| {
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
                panic!("missing function")
            };
            let body = hir.body(function.body);
            let FunctionBody::Expression(root) = function.body_form else {
                panic!("missing body")
            };
            let ExprKind::Match(value) = body.expression(root).unwrap().kind() else {
                panic!("missing match")
            };
            reset();
            assert!(matches!(
                resolver
                    .visit_pattern_heads(module, source, body, value.arms[0].pattern)
                    .unwrap(),
                VisitOutcome::Unperformed
            ));
            assert_eq!(work(), Work::default());
            assert_eq!(resolver.facts.iter().count(), 0);
            assert!(!resolver.diagnostics.has_errors());
        },
    );
}

#[test]
fn shared_recovery_roots_are_deduplicated_without_repeated_completion() {
    for width in [1usize, 8] {
        for count in [0usize, 1, 2, 3, 8, 128] {
            let prefix = (0..width)
                .map(|i| format!("fn damaged{i}() -> Unit = ,\n"))
                .collect::<String>();
            let heads = (0..count)
                .map(|i| format!("Missing{i}()"))
                .collect::<Vec<_>>();
            let pattern = if count == 0 {
                "()".into()
            } else {
                format!("({},)", heads.join(","))
            };
            let text = format!(
                "{prefix}fn probe(value: Unit) -> Unit = match value {{ {pattern} => () }}\n"
            );
            with_resolver(&text, &Support(true), |resolver, source| {
                let module = resolver.collection.module_for_file(source.file()).unwrap();
                let hir = source.hir().unwrap();
                let function = hir
                    .items()
                    .find_map(|(_, item)| match item.kind() {
                        ItemKind::Function(f) if f.name.text() == Some("probe") => Some(f),
                        _ => None,
                    })
                    .unwrap();
                let body = hir.body(function.body);
                let FunctionBody::Expression(root) = function.body_form else {
                    panic!("missing body")
                };
                let ExprKind::Match(value) = body.expression(root).unwrap().kind() else {
                    panic!("missing match")
                };
                reset();
                let outcome = resolver
                    .visit_pattern_heads(module, source, body, value.arms[0].pattern)
                    .unwrap();
                assert_eq!(work(), expected_work(count), "{width}/{count}");
                assert_eq!(matches!(outcome, VisitOutcome::Poisoned(_)), count > 0);
                assert_eq!(resolver.facts.iter().count(), count);
                let snapshot =
                    std::mem::replace(resolver.diagnostics, DiagnosticStore::new()).finalize(None);
                assert_eq!(snapshot.diagnostics().len(), width);
            });
        }
    }
}

#[test]
fn a_wrong_body_parameter_stops_before_later_activation_or_completion() {
    with_resolver(
        "fn probe(x: i64, x: i64, later: i64) -> Unit = ()\nfn other(z: i64) -> Unit = ()\n",
        &Support(true),
        |resolver, source| {
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
                panic!("missing function")
            };
            let ItemKind::Function(other) = hir.item(hir.roots()[1]).kind() else {
                panic!("missing other function")
            };
            let body = hir.body(function.body);
            let parameters = [
                function.parameters[0].clone(),
                function.parameters[1].clone(),
                other.parameters[0].clone(),
                function.parameters[2].clone(),
            ];
            resolver
                .push_scope(module, body.id(), ScopeKind::CallableParameters)
                .unwrap();
            reset();
            assert_eq!(
                resolver
                    .visit_parameters(module, source, hir, body, &parameters)
                    .err(),
                Some(NameResolutionError::WrongBodyPattern(
                    other.parameters[0].pattern
                ))
            );
            assert_eq!(work(), Work::default());
            assert_eq!(resolver.locals.len(), 1);
            assert_eq!(resolver.locals[0].name.text(), "x");
            assert_eq!(resolver.active_scopes.len(), 1);
            let snapshot =
                std::mem::replace(resolver.diagnostics, DiagnosticStore::new()).finalize(None);
            assert_eq!(snapshot.diagnostics().len(), 1);
            assert_eq!(
                snapshot.diagnostics()[0].code().as_str(),
                "LOCAL_BINDING_REDECLARATION"
            );
        },
    );
}

#[test]
fn a_foreign_pattern_root_stops_before_the_next_constructor() {
    with_resolver(
        "fn probe(value: Unit) -> Unit = match value { (Missing0(), Missing1(), Missing2()) => () }\n",
        &Support(true),
        |resolver, source| {
            let module = resolver.collection.module_for_file(source.file()).unwrap();
            let hir = source.hir().unwrap();
            let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
                panic!("missing function")
            };
            let body = hir.body(function.body);
            let FunctionBody::Expression(root) = function.body_form else {
                panic!("missing body")
            };
            let ExprKind::Match(value) = body.expression(root).unwrap().kind() else {
                panic!("missing match")
            };
            let pattern_id = value.arms[0].pattern;
            let PatternKind::Tuple(elements) = body.pattern(pattern_id).unwrap().kind() else {
                panic!("missing tuple")
            };
            let PatternElement::Pattern(second) = elements[1] else {
                panic!("missing second head")
            };
            let PatternKind::Variant { path, .. } = body.pattern(second).unwrap().kind() else {
                panic!("missing constructor")
            };
            let name = &path.segments()[0];
            let mut foreign_store = DiagnosticStore::new();
            let foreign = foreign_store
                .record_failure(DiagnosticFailure::Root(
                    DiagnosticRoot::boundary(
                        BoundaryRootKey::new("foreign").unwrap(),
                        Diagnostic::new(
                            DiagnosticCode::new("FOREIGN_ROOT").unwrap(),
                            Severity::Error,
                            DiagnosticPhase::new("resolve").unwrap(),
                            "foreign root",
                            PrimarySpan::Unavailable,
                        ),
                    )
                    .unwrap(),
                ))
                .unwrap();
            resolver
                .push_scope(module, body.id(), ScopeKind::CallableParameters)
                .unwrap();
            resolver
                .activate_binding(
                    BindingIntroduction {
                        name: NameKey::from_hir(name).unwrap(),
                        origins: vec![name.origin()],
                    },
                    BindingActivation::Poisoned(foreign),
                )
                .unwrap();
            reset();
            assert_eq!(
                resolver
                    .visit_pattern_heads(module, source, body, pattern_id)
                    .err(),
                Some(NameResolutionError::DiagnosticStore(
                    DiagnosticStoreError::ForeignPoison
                ))
            );
            assert_eq!(
                work(),
                Work {
                    pairs: 1,
                    includes: 0,
                    finishes: 0
                }
            );
            assert_eq!(resolver.facts.iter().count(), 2);
            assert_eq!(resolver.active_scopes.len(), 1);
            let snapshot =
                std::mem::replace(resolver.diagnostics, DiagnosticStore::new()).finalize(None);
            assert_eq!(snapshot.diagnostics().len(), 1);
            assert_eq!(snapshot.diagnostics()[0].code().as_str(), "NAME_NOT_FOUND");
        },
    );
}

use super::*;
use crate::body_types::checker::test_support::with_checker;
use crate::body_types::checker::unit_outcome_tests::{Work, reset, work};
use eglur_diagnostics::DiagnosticFailure;

#[test]
fn tuple_binding_completes_each_pattern_once_and_preserves_later_locals() {
    for count in [8_usize, 32, 128] {
        for mode in ["unit", "mixed", "grouped", "nested", "mut", "known"] {
            let pattern = (0..count)
                .map(|index| match mode {
                    "known" => format!("v{index}"),
                    "mixed" | "mut" if index % 2 == 0 => format!("v{index}"),
                    "grouped" => "((()))".into(),
                    "nested" => "((), ())".into(),
                    _ => "()".into(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let text = format!("fn f(x: i64) -> Unit {{ let ({pattern}) = 0\nyield () }}\n");
            with_checker(&text, |checker| {
                let owner = checker
                    .collection
                    .modules()
                    .flat_map(|m| m.declarations())
                    .find(|d| d.name().is_some_and(|name| name.text() == "f"))
                    .unwrap()
                    .id();
                let (module, source, hir, item) = checker.source_item(owner).unwrap();
                let ItemKind::Function(function) = item.kind() else {
                    panic!("function expected")
                };
                let body = hir.body(function.body);
                let pattern = body
                    .statements()
                    .find_map(|(_, statement)| match statement.kind() {
                        StmtKind::Let { pattern, .. } => Some(*pattern),
                        _ => None,
                    })
                    .unwrap();
                let integer = checker.primitive(PrimitiveType::I64).unwrap();
                let child = if mode == "nested" {
                    checker
                        .signatures
                        .types_mut()
                        .tuple([integer, integer])
                        .unwrap()
                } else {
                    integer
                };
                let ty = checker
                    .signatures
                    .types_mut()
                    .tuple(vec![child; count])
                    .unwrap();
                let mutability = if mode == "mut" {
                    Mutability::Mutable
                } else {
                    Mutability::Immutable
                };
                checker.current_owner = Some(owner);
                let region = ControlRegionKey::Callable(owner);
                checker.enter_control_region(region.clone()).unwrap();
                let checkpoint = checker.diagnostics.checkpoint();
                reset();
                let outcome = checker
                    .bind_pattern(
                        module,
                        source,
                        body,
                        pattern,
                        ty,
                        PatternBindingMode::Let(mutability),
                    )
                    .unwrap();
                let (inputs, completed, errors) = match mode {
                    "known" => (0, 0, 0),
                    "mixed" | "mut" => (count / 2, 1, count / 2),
                    "nested" => (3 * count, count + 1, 2 * count),
                    _ => (count, 1, count),
                };
                assert_eq!(work(), Work { inputs, completed }, "{mode}/{count}");
                assert_eq!(
                    checker.locals.len(),
                    match mode {
                        "known" => count,
                        "mixed" | "mut" => count / 2,
                        _ => 0,
                    }
                );
                for fact in checker.locals.facts() {
                    let SemanticFact::Known(local) = fact else {
                        panic!("known local expected")
                    };
                    assert_eq!(local.ty, integer);
                    assert_eq!(local.mutability, mutability);
                    assert_eq!(local.region, region);
                }
                if let Outcome::Poisoned(poison) = outcome {
                    let span = source
                        .origin_span(body.pattern(pattern).unwrap().origin())
                        .unwrap();
                    let all = checker
                        .diagnostics
                        .roots_since_within(&checkpoint, span)
                        .unwrap()
                        .unwrap();
                    checker
                        .diagnostics
                        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                        .unwrap();
                    checker
                        .diagnostics
                        .select_preceding_roots(&poison, &all)
                        .unwrap();
                } else {
                    assert!(matches!(outcome, Outcome::Known(())));
                    assert_eq!(errors, 0);
                }
                checker.leave_control_region(&region).unwrap();
                assert!(checker.region_stack.is_empty());
                let snapshot =
                    std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
                assert_eq!(snapshot.diagnostics().len(), errors);
                assert!(
                    snapshot
                        .diagnostics()
                        .iter()
                        .all(|d| d.code().as_str() == "LET_PATTERN_TYPE_MISMATCH")
                );
            });
        }
    }
}

#[test]
fn tuple_rest_type_and_arity_guards_reject_before_completion_or_binding() {
    for mode in ["rest", "type", "arity"] {
        let pattern = if mode == "rest" {
            "(left, ..)"
        } else {
            "(left, right)"
        };
        let text = format!("fn f() -> Unit {{ let {pattern} = 0\nyield () }}\n");
        with_checker(&text, |checker| {
            let owner = checker
                .collection
                .modules()
                .flat_map(|m| m.declarations())
                .next()
                .unwrap()
                .id();
            let (module, source, hir, item) = checker.source_item(owner).unwrap();
            let ItemKind::Function(function) = item.kind() else {
                panic!("function expected")
            };
            let body = hir.body(function.body);
            let pattern = body
                .statements()
                .find_map(|(_, s)| match s.kind() {
                    StmtKind::Let { pattern, .. } => Some(*pattern),
                    _ => None,
                })
                .unwrap();
            let integer = checker.primitive(PrimitiveType::I64).unwrap();
            let ty = if mode == "type" {
                integer
            } else {
                checker.signatures.types_mut().tuple([integer]).unwrap()
            };
            checker.current_owner = Some(owner);
            let region = ControlRegionKey::Callable(owner);
            checker.enter_control_region(region.clone()).unwrap();
            reset();
            assert!(matches!(
                checker
                    .bind_pattern(
                        module,
                        source,
                        body,
                        pattern,
                        ty,
                        PatternBindingMode::Let(Mutability::Mutable)
                    )
                    .unwrap(),
                Outcome::Poisoned(_)
            ));
            assert_eq!(work(), Work::default());
            assert_eq!(checker.locals.len(), 0);
            checker.leave_control_region(&region).unwrap();
            let snapshot =
                std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
            assert_eq!(snapshot.diagnostics().len(), 1);
            assert_eq!(
                snapshot.diagnostics()[0].code().as_str(),
                if mode == "rest" {
                    "LET_PATTERN_UNSUPPORTED"
                } else {
                    "LET_PATTERN_TYPE_MISMATCH"
                }
            );
        });
    }
}

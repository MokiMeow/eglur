use eglur_hir::{CallSyntax, DesugaringKind, ExprKind, HirFile, ItemKind, Mutability, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

fn lower_text(text: &str, success: bool) -> HirFile {
    let source = SourceFile::new(FileId::new(8), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8 source must parse")
    };
    assert_eq!(parsed.is_success(), success, "{:?}", parsed.diagnostics());
    lower(&parsed)
}

fn lower_expression(expression: &str) -> HirFile {
    lower_text(&format!("fn f() -> Unit = {expression}\n"), true)
}

fn count(hir: &HirFile, predicate: impl Fn(&ExprKind) -> bool) -> usize {
    hir.bodies()
        .flat_map(|(_, body)| body.expressions())
        .filter(|(_, expression)| predicate(expression.kind()))
        .count()
}

#[test]
fn deep_member_spines_lower_on_the_default_stack() {
    let hir = lower_expression(&format!("a{}", ".member".repeat(4000)));
    assert_eq!(
        count(&hir, |kind| matches!(kind, ExprKind::Member { .. })),
        4000
    );
    for (_, body) in hir.bodies() {
        for (id, expr) in body.expressions() {
            if let ExprKind::Member { receiver, member } = expr.kind() {
                assert_eq!(member.text(), Some("member"));
                assert!(receiver.local() < id.local());
                let child = body.expression(*receiver).unwrap();
                assert!(expr.origin().get() < child.origin().get());
                assert!(child.origin().get() < member.origin().get());
            }
        }
    }
}

#[test]
fn deep_projection_and_try_spines_keep_their_receivers() {
    let hir = lower_expression(&format!("&mut a{}", ".0?".repeat(4000)));
    assert_eq!(count(&hir, |kind| matches!(kind, ExprKind::Try(_))), 4000);
    assert_eq!(
        count(
            &hir,
            |kind| matches!(kind, ExprKind::TupleProjection { index, .. } if index.as_ref() == "0")
        ),
        4000
    );
    assert_eq!(
        count(&hir, |kind| matches!(
            kind,
            ExprKind::Reference {
                mutability: Mutability::Mutable,
                ..
            }
        )),
        1
    );
}

#[test]
fn deep_direct_and_explicit_type_calls_keep_one_target_per_call() {
    for (suffix, syntax, types) in [
        ("()", CallSyntax::Direct, 0),
        ("::[i64]()", CallSyntax::ExplicitTypeArguments, 1),
    ] {
        let hir = lower_expression(&format!("a{}", suffix.repeat(4000)));
        assert_eq!(count(&hir, |kind| matches!(kind, ExprKind::Call(_))), 4000);
        for (_, body) in hir.bodies() {
            for (id, expr) in body.expressions() {
                if let ExprKind::Call(call) = expr.kind() {
                    assert_eq!(call.syntax, syntax);
                    assert_eq!(call.type_arguments.len(), types);
                    assert!(call.arguments.is_empty());
                    assert!(call.callee.local() < id.local());
                    assert!(!call.partial_application);
                }
            }
        }
    }
}

#[test]
fn deep_then_spines_keep_callable_before_input_and_derived_origins() {
    let hir = lower_expression(&format!("a{}", ".then(f)".repeat(4000)));
    assert_eq!(count(&hir, |kind| matches!(kind, ExprKind::Call(_))), 4000);
    for (_, body) in hir.bodies() {
        for (id, expr) in body.expressions() {
            if let ExprKind::Call(call) = expr.kind() {
                assert_eq!(call.syntax, CallSyntax::PostfixThen);
                assert_eq!(call.arguments.len(), 1);
                let argument = &call.arguments[0];
                assert!(call.callee.local() < argument.value.local());
                assert!(argument.value.local() < id.local());
                assert_eq!(
                    hir.origin(expr.origin()).desugaring(),
                    Some(DesugaringKind::PostfixThen)
                );
                assert_eq!(
                    hir.origin(argument.origin).source(),
                    hir.origin(body.expression(argument.value).unwrap().origin())
                        .source()
                );
            }
        }
    }
}

#[test]
fn deep_dtype_and_reduce_by_spines_keep_typed_candidates() {
    for suffix in ["[i64]()", ".reduce_by(f)", ".reduce_by((x, f))"] {
        let hir = lower_expression(&format!("a{}", suffix.repeat(4000)));
        let dtype = suffix.starts_with('[');
        assert_eq!(
            count(&hir, |kind| if dtype {
                matches!(kind, ExprKind::DTypeCall(_))
            } else {
                matches!(kind, ExprKind::ReduceByCandidate(_))
            }),
            4000
        );
        for (_, body) in hir.bodies() {
            for (id, expr) in body.expressions() {
                if let ExprKind::ReduceByCandidate(candidate) = expr.kind() {
                    assert!(candidate.receiver.local() < id.local());
                    assert!(candidate.ordinary.is_some());
                    assert!(candidate.single_reducer.is_some());
                    assert_eq!(candidate.ordered_phases.is_some(), suffix.contains("(x,"));
                }
            }
        }
    }
}

#[test]
fn deep_mixed_spines_and_trailing_closures_compose_without_recursive_hops() {
    let hir = lower_expression(&format!(
        "a{}",
        ".member().0?.then(f)[i64]().reduce_by(f)".repeat(4000)
    ));
    for predicate in [
        (|kind: &ExprKind| matches!(kind, ExprKind::Try(_))) as fn(&ExprKind) -> bool,
        |kind| matches!(kind, ExprKind::DTypeCall(_)),
        |kind| matches!(kind, ExprKind::ReduceByCandidate(_)),
    ] {
        assert_eq!(count(&hir, predicate), 4000);
    }
    let hir = lower_expression(&format!("a{}", " { |x| x }".repeat(4000)));
    assert_eq!(
        count(
            &hir,
            |kind| matches!(kind, ExprKind::Call(call) if call.syntax == CallSyntax::TrailingArgument && call.arguments.len() == 1)
        ),
        4000
    );
}

#[test]
fn apply_inputs_and_callable_can_contain_deep_spines() {
    let chain = format!("a{}", ".member".repeat(4000));
    let hir = lower_expression(&format!("({chain}, _).apply({chain})"));
    assert_eq!(
        count(&hir, |kind| matches!(kind, ExprKind::Member { .. })),
        8000
    );
    for (_, body) in hir.bodies() {
        for (_, expr) in body.expressions() {
            if let ExprKind::Call(call) = expr.kind() {
                assert_eq!(call.syntax, CallSyntax::PostfixApply);
                assert_eq!(call.arguments.len(), 2);
                assert!(call.partial_application);
                assert!(call.callee.local() < call.arguments[0].value.local());
                assert!(call.arguments[0].value.local() < call.arguments[1].value.local());
            }
        }
    }
    assert_eq!(count(&hir, |kind| matches!(kind, ExprKind::Call(_))), 1);
}

#[test]
fn declined_apply_does_not_allocate_an_orphan_callable() {
    let hir = lower_expression("a.apply(f)");
    assert_eq!(count(&hir, |_| true), 4);
    assert_eq!(
        count(
            &hir,
            |kind| matches!(kind, ExprKind::Call(call) if call.syntax == CallSyntax::Direct)
        ),
        1
    );
    assert_eq!(count(&hir, |kind| matches!(kind, ExprKind::Name(_))), 2);
}

#[test]
fn malformed_deep_postfix_keeps_shallow_recovery_and_parser_causes() {
    let root_names = |hir: &HirFile| {
        hir.roots()
            .iter()
            .map(|root| match hir.item(*root).kind() {
                ItemKind::Function(function) => function.name.text().map(str::to_owned),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for suffix in [".", ".then(", "::[i64](", ".then(,)", "::[i64](,)"] {
        let text = format!(
            "fn f() -> Unit = a{}{suffix}\nfn next() -> Unit = ()\n",
            ".member()".repeat(4000)
        );
        let hir = lower_text(&text, false);
        assert!(!hir.invalid_occurrences().is_empty());
        assert!(
            hir.invalid_occurrences()
                .iter()
                .any(|invalid| !invalid.syntax_causes().is_empty())
        );
        let shallow = lower_text(
            &format!("fn f() -> Unit = a.member(){suffix}\nfn next() -> Unit = ()\n"),
            false,
        );
        assert_eq!(root_names(&hir), root_names(&shallow), "{suffix}");
        if suffix.ends_with(')') {
            assert!(root_names(&hir).contains(&Some("next".to_owned())));
        }
    }
}

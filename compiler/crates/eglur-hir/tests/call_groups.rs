use eglur_hir::{
    CallExpr, CallSyntax, DesugaringKind, ExprId, ExprKind, FunctionBody, HirFile, ItemKind, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

fn lower_expression(expression: &str) -> (String, HirFile, ExprId) {
    let text = format!("fn sample() -> Unit = {expression}\n");
    let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parse) = parse(&source) else {
        panic!("test source should reach the parser");
    };
    assert!(parse.is_success(), "{text}: {:?}", parse.diagnostics());
    assert_eq!(parse.syntax().text(), text);

    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty(), "{text}");
    let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
        panic!("source should contain a function");
    };
    let FunctionBody::Expression(root) = function.body_form else {
        panic!("function should retain expression-body form");
    };
    (text, hir, root)
}

fn call_at(hir: &HirFile, id: ExprId) -> &CallExpr {
    let ExprKind::Call(call) = hir.body(id.body()).expression(id).unwrap().kind() else {
        panic!("expected a call");
    };
    assert_eq!(call.callee.body(), id.body());
    assert!(
        call.arguments
            .iter()
            .all(|arg| arg.value.body() == id.body())
    );
    call
}

fn written_expression<'a>(text: &'a str, hir: &HirFile, id: ExprId) -> &'a str {
    let expression = hir.body(id.body()).expression(id).unwrap();
    let range = hir.origin(expression.origin()).source().range();
    text[range.start().as_usize()..range.end().as_usize()].trim()
}

#[test]
fn grouped_tuple_apply_preserves_arity_order_and_surface_origin() {
    for source in [
        "((1, 2)).apply(f)",
        "(((1, 2))).apply(f)",
        "((1, 2).apply)(f)",
        "((((1, 2)).apply))(f)",
        "(\n /* tuple */ (1, 2)\n).apply(f)",
        "(\r\n(1, 2) // tuple\r\n).apply(f)",
    ] {
        let (text, hir, root) = lower_expression(source);
        let call = call_at(&hir, root);
        assert_eq!(call.syntax, CallSyntax::PostfixApply, "{source}");
        assert!(!call.partial_application);
        assert_eq!(written_expression(&text, &hir, call.callee), "f");
        assert_eq!(
            call.arguments
                .iter()
                .map(|arg| written_expression(&text, &hir, arg.value))
                .collect::<Vec<_>>(),
            ["1", "2"]
        );
        let origin = hir.origin(hir.body(root.body()).expression(root).unwrap().origin());
        assert_eq!(origin.desugaring(), Some(DesugaringKind::PostfixApply));
        assert_eq!(written_expression(&text, &hir, root), source);
    }
}

#[test]
fn tuple_contents_and_grouped_callables_are_not_flattened() {
    for (source, arguments) in [
        ("(((1), (2, 3))).apply((f))", vec!["(1)", "(2, 3)"]),
        ("(((1, 2),)).apply((f))", vec!["(1, 2)"]),
        ("(((1, 2),)).then((f))", vec!["(((1, 2),))"]),
    ] {
        let (text, hir, root) = lower_expression(source);
        let call = call_at(&hir, root);
        assert_eq!(written_expression(&text, &hir, call.callee), "(f)");
        assert!(matches!(
            hir.body(root.body())
                .expression(call.callee)
                .unwrap()
                .kind(),
            ExprKind::Group(_)
        ));
        assert_eq!(
            call.arguments
                .iter()
                .map(|arg| written_expression(&text, &hir, arg.value))
                .collect::<Vec<_>>(),
            arguments,
            "{source}"
        );
    }
}

#[test]
fn grouped_then_callee_keeps_the_complete_receiver() {
    for source in ["((1, 2).then)(f)", "((((1, 2)).then))(f)"] {
        let (text, hir, root) = lower_expression(source);
        let call = call_at(&hir, root);
        assert_eq!(call.syntax, CallSyntax::PostfixThen, "{source}");
        assert_eq!(call.arguments.len(), 1);
        let receiver = call.arguments[0].value;
        assert_eq!(
            written_expression(&text, &hir, receiver),
            if source == "((1, 2).then)(f)" {
                "(1, 2)"
            } else {
                "((1, 2))"
            }
        );
        let origin = hir.origin(hir.body(root.body()).expression(root).unwrap().origin());
        assert_eq!(origin.desugaring(), Some(DesugaringKind::PostfixThen));
        assert_eq!(written_expression(&text, &hir, root), source);
    }
}

#[test]
fn grouped_holes_preserve_immediate_call_boundaries() {
    for (source, partial, total_partial) in [
        ("(_)(1)", true, 1),
        ("((_))(1)", true, 1),
        ("f((_))", true, 1),
        ("f::[i64](first = (_), second = 2)", true, 1),
        ("(_) { || 1 }", true, 1),
        ("1.then((_))", true, 1),
        ("((1, 2)).apply((_))", true, 1),
        ("(((_), 2)).apply(f)", true, 1),
        ("f((g(_)))", false, 1),
        ("f(((_ + 1)))", false, 0),
        ("f(((_, 1)))", false, 0),
        ("f(({ |_| 1 }))", false, 0),
        ("f((()))", false, 0),
    ] {
        let (text, hir, root) = lower_expression(source);
        let call = call_at(&hir, root);
        assert_eq!(call.partial_application, partial, "{source}");
        assert_eq!(
            hir.bodies()
                .flat_map(|(_, body)| body.expressions())
                .filter(|(_, expr)| matches!(expr.kind(), ExprKind::Call(call) if call.partial_application))
                .count(),
            total_partial,
            "{source}"
        );
        if partial && call.syntax.call_desugaring().is_none() {
            let origin = hir.origin(hir.body(root.body()).expression(root).unwrap().origin());
            assert_eq!(
                origin.desugaring(),
                Some(DesugaringKind::HolePartialApplication)
            );
            assert_eq!(written_expression(&text, &hir, root), source);
        }
    }
}

#[test]
fn grouping_does_not_promote_non_intrinsic_calls() {
    for source in [
        "((1, 2)).other(f)",
        "(receiver).apply(f)",
        "().apply(f)",
        "({ yield (1, 2) }).apply(f)",
        "((1, 2)).apply(f, g)",
        "((1, 2)).apply(callable = f)",
        "((1, 2).other)(f)",
    ] {
        let (_, hir, root) = lower_expression(source);
        assert_eq!(call_at(&hir, root).syntax, CallSyntax::Direct, "{source}");
    }
}

#[test]
fn ordinary_reduce_by_candidates_observe_grouped_holes_only() {
    for (source, partial) in [
        ("receiver.reduce_by((_))", true),
        ("receiver.reduce_by((reducer))", false),
        ("receiver.reduce_by((reducer(_)))", false),
    ] {
        let (_, hir, root) = lower_expression(source);
        let ExprKind::ReduceByCandidate(candidate) =
            hir.body(root.body()).expression(root).unwrap().kind()
        else {
            panic!("expected reduce_by candidates");
        };
        let ordinary = candidate.ordinary.as_ref().unwrap();
        assert_eq!(ordinary.call.partial_application, partial, "{source}");
        assert_eq!(
            hir.origin(ordinary.origin).desugaring(),
            Some(DesugaringKind::ReduceByOrdinaryCandidate)
        );
    }
}

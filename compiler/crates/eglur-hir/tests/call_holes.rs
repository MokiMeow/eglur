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

fn call_at(hir: &HirFile, expression: ExprId) -> &CallExpr {
    let ExprKind::Call(call) = hir
        .body(expression.body())
        .expression(expression)
        .unwrap()
        .kind()
    else {
        panic!("expected an ordinary or desugared call");
    };
    assert_eq!(call.callee.body(), expression.body());
    assert!(
        call.arguments
            .iter()
            .all(|argument| argument.value.body() == expression.body())
    );
    call
}

fn child_spellings<'a>(hir: &'a HirFile, call: &CallExpr) -> Vec<&'a str> {
    std::iter::once(call.callee)
        .chain(call.arguments.iter().map(|argument| argument.value))
        .map(
            |id| match hir.body(id.body()).expression(id).unwrap().kind() {
                ExprKind::Hole => "_",
                ExprKind::Name(path) => path.last().unwrap().text().unwrap(),
                ExprKind::Literal(literal) => literal.spelling(),
                kind => panic!("unexpected immediate call child: {kind:?}"),
            },
        )
        .collect()
}

#[test]
fn immediate_callee_holes_mark_the_complete_partial_call_surface() {
    for source in [
        "_()",
        "_(1)",
        "_(1, 2)",
        "_(first = 1)",
        "_(1, remaining = _)",
        "_ { || 1 }",
    ] {
        let (text, hir, root) = lower_expression(source);
        let call = call_at(&hir, root);
        assert!(call.partial_application, "{source}");
        assert!(matches!(
            hir.body(root.body())
                .expression(call.callee)
                .unwrap()
                .kind(),
            ExprKind::Hole
        ));

        let expression = hir.body(root.body()).expression(root).unwrap();
        let origin = hir.origin(expression.origin());
        assert_eq!(
            origin.desugaring(),
            Some(DesugaringKind::HolePartialApplication)
        );
        let span = origin.source().range();
        assert_eq!(
            text[span.start().as_usize()..span.end().as_usize()].trim(),
            source
        );
    }
}

#[test]
fn direct_and_postfix_call_forms_agree_on_partial_classification() {
    for (direct, postfix, partial, syntax) in [
        ("_(1)", "1.then(_)", true, CallSyntax::PostfixThen),
        ("_(1, 2)", "(1, 2).apply(_)", true, CallSyntax::PostfixApply),
        ("f(_)", "_.then(f)", true, CallSyntax::PostfixThen),
        ("f(1, _)", "(1, _).apply(f)", true, CallSyntax::PostfixApply),
        ("_(_)", "_.then(_)", true, CallSyntax::PostfixThen),
        ("f(1)", "1.then(f)", false, CallSyntax::PostfixThen),
        (
            "f(1, 2)",
            "(1, 2).apply(f)",
            false,
            CallSyntax::PostfixApply,
        ),
    ] {
        let (_, direct_hir, direct_root) = lower_expression(direct);
        let (_, postfix_hir, postfix_root) = lower_expression(postfix);
        let direct_call = call_at(&direct_hir, direct_root);
        let postfix_call = call_at(&postfix_hir, postfix_root);
        assert_eq!(direct_call.syntax, CallSyntax::Direct);
        assert_eq!(postfix_call.syntax, syntax);
        assert_eq!(direct_call.partial_application, partial, "{direct}");
        assert_eq!(postfix_call.partial_application, partial, "{postfix}");
        assert_eq!(
            child_spellings(&direct_hir, direct_call),
            child_spellings(&postfix_hir, postfix_call)
        );

        let expression = postfix_hir
            .body(postfix_root.body())
            .expression(postfix_root)
            .unwrap();
        assert_eq!(
            postfix_hir.origin(expression.origin()).desugaring(),
            syntax.call_desugaring()
        );
    }
}

#[test]
fn nested_partial_calls_and_lambdas_keep_separate_hole_boundaries() {
    for (source, outer_partial, total_partial) in [
        ("f(g(_), 1)", false, 1),
        ("f(_(1))", false, 1),
        ("f({ |x| x })", false, 0),
        ("f({ |_| 1 })", false, 0),
        ("f({ g(_) })", false, 1),
        ("f({ || g(_) })", false, 1),
        ("_(_(1))", true, 2),
        ("_(f(1))", true, 1),
    ] {
        let (_, hir, root) = lower_expression(source);
        assert_eq!(
            call_at(&hir, root).partial_application,
            outer_partial,
            "{source}"
        );
        assert_eq!(
            hir.bodies()
                .flat_map(|(_, body)| body.expressions())
                .filter(|(_, expression)| matches!(expression.kind(), ExprKind::Call(call) if call.partial_application))
                .count(),
            total_partial,
            "{source}"
        );
    }
}

#[test]
fn partial_calls_keep_named_and_explicit_type_arguments() {
    let (_, hir, root) = lower_expression("f::[i64](first = _, second = 2)");
    let call = call_at(&hir, root);
    assert!(call.partial_application);
    assert_eq!(call.syntax, CallSyntax::ExplicitTypeArguments);
    assert_eq!(call.type_arguments.len(), 1);
    assert_eq!(
        call.arguments
            .iter()
            .map(|argument| argument.name.as_ref().unwrap().text().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(child_spellings(&hir, call), ["f", "_", "2"]);
}

#[test]
fn trailing_argument_origins_do_not_become_partial_call_origins() {
    for (source, partial) in [("f { || 1 }", false), ("_ { || 1 }", true)] {
        let (_, hir, root) = lower_expression(source);
        let call = call_at(&hir, root);
        assert_eq!(call.syntax, CallSyntax::TrailingArgument);
        assert_eq!(call.partial_application, partial);
        assert_eq!(call.arguments.len(), 1);
        let argument = &call.arguments[0];
        assert_eq!(
            hir.origin(argument.origin).desugaring(),
            Some(DesugaringKind::TrailingArgument)
        );
        assert!(matches!(
            hir.body(root.body())
                .expression(argument.value)
                .unwrap()
                .kind(),
            ExprKind::Lambda(_)
        ));
    }
}

#[test]
fn ordinary_reduce_by_candidates_classify_only_immediate_argument_holes() {
    for (source, partial) in [
        ("receiver.reduce_by(_)", true),
        ("receiver.reduce_by(reducer)", false),
        ("receiver.reduce_by(reducer(_))", false),
    ] {
        let (_, hir, root) = lower_expression(source);
        let expression = hir.body(root.body()).expression(root).unwrap();
        let ExprKind::ReduceByCandidate(candidate) = expression.kind() else {
            panic!("source should retain its reduce_by candidates");
        };
        let ordinary = candidate
            .ordinary
            .as_ref()
            .expect("ordinary call should remain a candidate");
        assert_eq!(ordinary.call.partial_application, partial, "{source}");
        assert!(matches!(
            hir.body(root.body())
                .expression(ordinary.call.callee)
                .unwrap()
                .kind(),
            ExprKind::Member { .. }
        ));
        assert_eq!(
            hir.origin(ordinary.origin).desugaring(),
            Some(DesugaringKind::ReduceByOrdinaryCandidate)
        );
    }
}

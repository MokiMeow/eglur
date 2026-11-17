use eglur_hir::{
    CallSyntax, ExprKind, LambdaSyntax, Mutability, PatternKind, SourceOrigin, StmtKind,
    TypeRefKind,
};
use eglur_syntax::{SyntaxElement, SyntaxKind};

use super::LoweredCallable;

#[test]
fn explicit_parameters_keep_binding_tokens_optional_types_and_body_ownership() {
    let spelling =
        "{ |first: &Alpha, _: (Beta, Gamma), untyped, last: Seq[Delta], _|\nlet local = 68\n79\n}";
    let source = LoweredCallable::new(spelling);
    let lambda = source.lambda(
        source.root.body(),
        source.root,
        spelling,
        LambdaSyntax::Explicit,
    );
    let expected = [
        ("first: &Alpha", Some("first")),
        ("_: (Beta, Gamma)", None),
        ("untyped", Some("untyped")),
        ("last: Seq[Delta]", Some("last")),
        ("_", None),
    ];
    assert_eq!(lambda.parameters.len(), expected.len());
    for (parameter, (spelling, binding)) in lambda.parameters.iter().zip(expected) {
        source.written(parameter.origin, SyntaxKind::Parameter, spelling);
        let node = source.node(SyntaxKind::Parameter, spelling);
        let token = node
            .children_with_tokens()
            .find_map(|element| match element {
                SyntaxElement::Token(token) if token.text() == binding.unwrap_or("_") => {
                    Some(token)
                }
                _ => None,
            })
            .unwrap();
        let pattern = source
            .hir
            .body(lambda.body)
            .pattern(parameter.pattern)
            .unwrap();
        assert_eq!(
            source.hir.origin(pattern.origin()),
            SourceOrigin::Written(token.span())
        );
        match (pattern.kind(), binding) {
            (PatternKind::Binding(name), Some(expected)) => {
                assert_eq!(name.text(), Some(expected));
                assert!(!name.is_bang());
                assert_eq!(
                    source.hir.origin(name.origin()),
                    SourceOrigin::Written(token.span())
                );
            }
            (PatternKind::Wildcard, None) => {}
            _ => panic!("parameter binding/wildcard role changed"),
        }
    }

    let TypeRefKind::Reference {
        mutability,
        referent,
    } = source.type_kind(
        lambda.parameters[0].ty.unwrap(),
        SyntaxKind::RefType,
        "&Alpha",
    )
    else {
        panic!("first parameter annotation must remain a reference");
    };
    assert_eq!(*mutability, Mutability::Immutable);
    source.type_leaf(*referent, "Alpha");
    let TypeRefKind::Tuple(elements) = source.type_kind(
        lambda.parameters[1].ty.unwrap(),
        SyntaxKind::TupleType,
        "(Beta, Gamma)",
    ) else {
        panic!("wildcard annotation must remain a tuple");
    };
    assert_eq!(elements.len(), 2);
    source.type_leaf(elements[0], "Beta");
    source.type_leaf(elements[1], "Gamma");
    assert!(lambda.parameters[2].ty.is_none());
    let elements = source.type_path(lambda.parameters[3].ty.unwrap(), "Seq[Delta]", &["Seq"]);
    assert_eq!(elements.len(), 1);
    source.type_leaf(elements[0], "Delta");
    assert!(lambda.parameters[4].ty.is_none());

    assert_eq!(lambda.statements.len(), 1);
    let statement = source
        .hir
        .body(lambda.body)
        .statement(lambda.statements[0])
        .unwrap();
    source.written(statement.origin(), SyntaxKind::LetStmt, "let local = 68");
    let StmtKind::Let {
        initializer: Some(initializer),
        ..
    } = statement.kind()
    else {
        panic!("expected local initialized binding");
    };
    source.integer(lambda.body, *initializer, "68");
    source.integer(lambda.body, lambda.tail.unwrap(), "79");
}

#[test]
fn sibling_lambda_arguments_do_not_share_parameter_or_tail_owners() {
    let spelling = "pair({ |left: Alpha| 15 }, second = { |right: Beta| 26 })";
    let source = LoweredCallable::new(spelling);
    let owner = source.root.body();
    let call = source.call(owner, source.root, spelling, CallSyntax::Direct);
    source.name(owner, call.callee, "pair", false);
    assert!(call.type_arguments.is_empty());
    assert_eq!(call.arguments.len(), 2);
    let first = source.argument(
        owner,
        &call.arguments[0],
        "{ |left: Alpha| 15 }",
        SyntaxKind::LambdaExpr,
        None,
    );
    let second = source.argument(
        owner,
        &call.arguments[1],
        "second = { |right: Beta| 26 }",
        SyntaxKind::NamedArgument,
        Some("second"),
    );
    let left = source.lambda(owner, first, "{ |left: Alpha| 15 }", LambdaSyntax::Explicit);
    let right = source.lambda(
        owner,
        second,
        "{ |right: Beta| 26 }",
        LambdaSyntax::Explicit,
    );
    assert_ne!(left.body, right.body);

    for (lambda, sibling, name, annotation, tail) in [
        (left, right, "left", "Alpha", "15"),
        (right, left, "right", "Beta", "26"),
    ] {
        assert_eq!(lambda.parameters.len(), 1);
        let parameter = &lambda.parameters[0];
        source.written(
            parameter.origin,
            SyntaxKind::Parameter,
            &format!("{name}: {annotation}"),
        );
        let pattern = source
            .hir
            .body(lambda.body)
            .pattern(parameter.pattern)
            .unwrap();
        let PatternKind::Binding(binding) = pattern.kind() else {
            panic!("expected binding parameter");
        };
        let token = source.identifier(binding, name, false);
        assert_eq!(
            source.hir.origin(pattern.origin()),
            SourceOrigin::Written(token)
        );
        source.type_leaf(parameter.ty.unwrap(), annotation);
        assert!(lambda.statements.is_empty());
        source.integer(lambda.body, lambda.tail.unwrap(), tail);
        assert!(
            source
                .hir
                .body(sibling.body)
                .pattern(parameter.pattern)
                .is_none()
        );
        assert!(
            source
                .hir
                .body(sibling.body)
                .expression(lambda.tail.unwrap())
                .is_none()
        );
    }
}

#[test]
fn empty_explicit_parameters_remain_distinct_from_implicit_lambda_syntax() {
    for spelling in ["{ || 37 }", "{ | | 37 }"] {
        let source = LoweredCallable::new(spelling);
        let lambda = source.lambda(
            source.root.body(),
            source.root,
            spelling,
            LambdaSyntax::Explicit,
        );
        assert!(lambda.parameters.is_empty());
        assert!(lambda.statements.is_empty());
        source.integer(lambda.body, lambda.tail.unwrap(), "37");
    }

    for (callee, bang, spelling, lambda_spelling) in [
        ("consume", false, "consume { _.payload }", "{ _.payload }"),
        ("spawn", true, "spawn! { 48 }", "{ 48 }"),
    ] {
        let source = LoweredCallable::new(spelling);
        let owner = source.root.body();
        let call = source.call(owner, source.root, spelling, CallSyntax::TrailingArgument);
        source.name(owner, call.callee, callee, bang);
        assert!(call.type_arguments.is_empty());
        assert_eq!(call.arguments.len(), 1);
        assert!(call.arguments[0].name.is_none());
        let lambda = source.lambda(
            owner,
            call.arguments[0].value,
            lambda_spelling,
            LambdaSyntax::Implicit,
        );
        assert!(lambda.parameters.is_empty());
        assert!(lambda.statements.is_empty());
        let tail = lambda.tail.unwrap();
        if bang {
            source.integer(lambda.body, tail, "48");
        } else {
            let ExprKind::Member {
                receiver,
                member: name,
            } = source.kind(lambda.body, tail, SyntaxKind::MemberExpr, "_.payload")
            else {
                panic!("expected placeholder member syntax");
            };
            source.identifier(name, "payload", false);
            assert!(matches!(
                source.kind(lambda.body, *receiver, SyntaxKind::HoleExpr, "_"),
                ExprKind::Hole
            ));
        }
    }
}

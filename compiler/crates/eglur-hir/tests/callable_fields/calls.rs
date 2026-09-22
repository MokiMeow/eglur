use eglur_hir::{
    CallSyntax, DesugaringKind, ExprKind, LambdaSyntax, Mutability, SourceOrigin, TypeRefKind,
};
use eglur_syntax::SyntaxKind;

use super::LoweredCallable;

#[test]
fn direct_calls_keep_callee_and_ordered_named_value_pairs() {
    let spelling = "dispatch!(11, nested(22, inner = 33), last = 44, flag = 55)";
    let source = LoweredCallable::new(spelling);
    let owner = source.root.body();
    let call = source.call(owner, source.root, spelling, CallSyntax::Direct);
    source.name(owner, call.callee, "dispatch", true);
    assert!(call.type_arguments.is_empty());
    assert_eq!(call.arguments.len(), 4);

    let first = source.argument(
        owner,
        &call.arguments[0],
        "11",
        SyntaxKind::LiteralExpr,
        None,
    );
    source.integer(owner, first, "11");
    let nested = source.argument(
        owner,
        &call.arguments[1],
        "nested(22, inner = 33)",
        SyntaxKind::CallExpr,
        None,
    );
    let inner = source.call(owner, nested, "nested(22, inner = 33)", CallSyntax::Direct);
    source.name(owner, inner.callee, "nested", false);
    assert!(inner.type_arguments.is_empty());
    assert_eq!(inner.arguments.len(), 2);
    let positional = source.argument(
        owner,
        &inner.arguments[0],
        "22",
        SyntaxKind::LiteralExpr,
        None,
    );
    source.integer(owner, positional, "22");
    let named = source.argument(
        owner,
        &inner.arguments[1],
        "inner = 33",
        SyntaxKind::NamedArgument,
        Some("inner"),
    );
    source.integer(owner, named, "33");

    for (argument, name, value) in [
        (&call.arguments[2], "last", "44"),
        (&call.arguments[3], "flag", "55"),
    ] {
        let id = source.argument(
            owner,
            argument,
            &format!("{name} = {value}"),
            SyntaxKind::NamedArgument,
            Some(name),
        );
        source.integer(owner, id, value);
    }
}

#[test]
fn explicit_type_calls_preserve_member_callee_and_heterogeneous_type_order() {
    let spelling = "service.convert!::[pkg::Envelope[Element], &mut Target](61, mode = 72)";
    let source = LoweredCallable::new(spelling);
    let owner = source.root.body();
    let call = source.call(
        owner,
        source.root,
        spelling,
        CallSyntax::ExplicitTypeArguments,
    );
    let ExprKind::Member {
        receiver,
        member: name,
    } = source.kind(
        owner,
        call.callee,
        SyntaxKind::MemberExpr,
        "service.convert!",
    )
    else {
        panic!("expected member callee");
    };
    source.name(owner, *receiver, "service", false);
    source.identifier(name, "convert", true);

    assert_eq!(call.type_arguments.len(), 2);
    let element = source.type_path(
        call.type_arguments[0],
        "pkg::Envelope[Element]",
        &["pkg", "Envelope"],
    );
    assert_eq!(element.len(), 1);
    source.type_leaf(element[0], "Element");
    let TypeRefKind::Reference {
        mutability,
        referent,
    } = source.type_kind(call.type_arguments[1], SyntaxKind::RefType, "&mut Target")
    else {
        panic!("expected reference as the second type argument");
    };
    assert_eq!(*mutability, Mutability::Mutable);
    source.type_leaf(*referent, "Target");

    assert_eq!(call.arguments.len(), 2);
    let first = source.argument(
        owner,
        &call.arguments[0],
        "61",
        SyntaxKind::LiteralExpr,
        None,
    );
    source.integer(owner, first, "61");
    let second = source.argument(
        owner,
        &call.arguments[1],
        "mode = 72",
        SyntaxKind::NamedArgument,
        Some("mode"),
    );
    source.integer(owner, second, "72");
}

#[test]
fn empty_value_lists_are_not_unit_arguments() {
    for (callee, syntax) in [
        ("build", CallSyntax::Direct),
        ("build::[Element]", CallSyntax::ExplicitTypeArguments),
    ] {
        for unit in [false, true] {
            let spelling = format!("{callee}({})", if unit { "()" } else { "" });
            let source = LoweredCallable::new(&spelling);
            let owner = source.root.body();
            let call = source.call(owner, source.root, &spelling, syntax);
            source.name(owner, call.callee, "build", false);
            if syntax == CallSyntax::Direct {
                assert!(call.type_arguments.is_empty());
            } else {
                assert_eq!(call.type_arguments.len(), 1);
                source.type_leaf(call.type_arguments[0], "Element");
            }
            assert_eq!(call.arguments.len(), usize::from(unit));
            if unit {
                let value =
                    source.argument(owner, &call.arguments[0], "()", SyntaxKind::UnitExpr, None);
                assert!(matches!(
                    source.kind(owner, value, SyntaxKind::UnitExpr, "()"),
                    ExprKind::Unit
                ));
            }
        }
    }
}

#[test]
fn catalog_calls_keep_owner_types_separate_from_member_and_value_arguments() {
    for (owner_names, member, bang) in [
        (vec!["catalog", "Store"], "insert", true),
        (vec!["Store"], "lookup", false),
    ] {
        let spelling = format!(
            "{}[Key, Seq[Element]].{member}{}(81, fallback = 92)",
            owner_names.join("::"),
            if bang { "!" } else { "" }
        );
        let source = LoweredCallable::new(&spelling);
        let owner = source.root.body();
        let ExprKind::CatalogCall(call) =
            source.kind(owner, source.root, SyntaxKind::CallExpr, &spelling)
        else {
            panic!("expected catalog-owner call");
        };
        source.path(&call.owner, &owner_names, false);
        source.identifier(&call.member, member, bang);
        assert_eq!(call.owner_arguments.len(), 2);
        source.type_leaf(call.owner_arguments[0], "Key");
        let elements = source.type_path(call.owner_arguments[1], "Seq[Element]", &["Seq"]);
        assert_eq!(elements.len(), 1);
        source.type_leaf(elements[0], "Element");
        assert_eq!(call.arguments.len(), 2);
        let first = source.argument(
            owner,
            &call.arguments[0],
            "81",
            SyntaxKind::LiteralExpr,
            None,
        );
        source.integer(owner, first, "81");
        let second = source.argument(
            owner,
            &call.arguments[1],
            "fallback = 92",
            SyntaxKind::NamedArgument,
            Some("fallback"),
        );
        source.integer(owner, second, "92");
    }
}

#[test]
fn catalog_empty_arguments_retain_the_owner_and_member() {
    let spelling = "pkg::Factory[Input, Output].make!()";
    let source = LoweredCallable::new(spelling);
    let ExprKind::CatalogCall(call) = source.kind(
        source.root.body(),
        source.root,
        SyntaxKind::CallExpr,
        spelling,
    ) else {
        panic!("expected catalog-owner call");
    };
    source.path(&call.owner, &["pkg", "Factory"], false);
    source.identifier(&call.member, "make", true);
    assert_eq!(call.owner_arguments.len(), 2);
    source.type_leaf(call.owner_arguments[0], "Input");
    source.type_leaf(call.owner_arguments[1], "Output");
    assert!(call.arguments.is_empty());
}

#[test]
fn dtype_calls_retain_callee_dtype_and_value_fields_before_eligibility_checks() {
    let spelling = "zeros[i32](13, fill = 24)";
    let source = LoweredCallable::new(spelling);
    let owner = source.root.body();
    let ExprKind::DTypeCall(call) = source.kind(
        owner,
        source.root,
        SyntaxKind::DTypeBracketCallExpr,
        spelling,
    ) else {
        panic!("expected dtype-bracket call");
    };
    source.name(owner, call.callee, "zeros", false);
    source.type_leaf(call.dtype, "i32");
    assert_eq!(call.arguments.len(), 2);
    let first = source.argument(
        owner,
        &call.arguments[0],
        "13",
        SyntaxKind::LiteralExpr,
        None,
    );
    source.integer(owner, first, "13");
    let second = source.argument(
        owner,
        &call.arguments[1],
        "fill = 24",
        SyntaxKind::NamedArgument,
        Some("fill"),
    );
    source.integer(owner, second, "24");
}

#[test]
fn empty_dtype_calls_keep_the_dtype_and_grouped_bang_callee() {
    let spelling = "(sample!)[f64]()";
    let source = LoweredCallable::new(spelling);
    let owner = source.root.body();
    let ExprKind::DTypeCall(call) = source.kind(
        owner,
        source.root,
        SyntaxKind::DTypeBracketCallExpr,
        spelling,
    ) else {
        panic!("expected dtype-bracket call");
    };
    let ExprKind::Group(inner) =
        source.kind(owner, call.callee, SyntaxKind::GroupExpr, "(sample!)")
    else {
        panic!("callee group must remain explicit in source HIR");
    };
    source.name(owner, *inner, "sample", true);
    source.type_leaf(call.dtype, "f64");
    assert!(call.arguments.is_empty());
}

#[test]
fn trailing_closure_preserves_the_preceding_typed_call_and_argument_origin() {
    let spelling = "factory::[Input, Output](35, mode = 46) { |item: Element| 57 }";
    let source = LoweredCallable::new(spelling);
    let owner = source.root.body();
    let outer = source.call(owner, source.root, spelling, CallSyntax::TrailingArgument);
    assert!(outer.type_arguments.is_empty());
    let inner = source.call(
        owner,
        outer.callee,
        "factory::[Input, Output](35, mode = 46)",
        CallSyntax::ExplicitTypeArguments,
    );
    source.name(owner, inner.callee, "factory", false);
    assert_eq!(inner.type_arguments.len(), 2);
    source.type_leaf(inner.type_arguments[0], "Input");
    source.type_leaf(inner.type_arguments[1], "Output");
    assert_eq!(inner.arguments.len(), 2);
    let first = source.argument(
        owner,
        &inner.arguments[0],
        "35",
        SyntaxKind::LiteralExpr,
        None,
    );
    source.integer(owner, first, "35");
    let second = source.argument(
        owner,
        &inner.arguments[1],
        "mode = 46",
        SyntaxKind::NamedArgument,
        Some("mode"),
    );
    source.integer(owner, second, "46");

    let [trailing] = outer.arguments.as_ref() else {
        panic!("expected one synthetic trailing argument");
    };
    assert!(trailing.name.is_none());
    let lambda_spelling = "{ |item: Element| 57 }";
    assert_eq!(
        source.hir.origin(trailing.origin),
        SourceOrigin::Derived {
            source: source.node(SyntaxKind::LambdaExpr, lambda_spelling).span(),
            kind: DesugaringKind::TrailingArgument,
        }
    );
    let lambda = source.lambda(
        owner,
        trailing.value,
        lambda_spelling,
        LambdaSyntax::Explicit,
    );
    assert_eq!(lambda.parameters.len(), 1);
    source.type_leaf(lambda.parameters[0].ty.unwrap(), "Element");
    assert!(lambda.statements.is_empty());
    source.integer(lambda.body, lambda.tail.unwrap(), "57");
}

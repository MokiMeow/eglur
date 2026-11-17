use eglur_hir::{ExprKind, Mutability, UnaryOperator};
use eglur_syntax::SyntaxKind;

use super::LoweredExpression;

#[test]
fn member_projection_and_try_chains_keep_each_receiver_and_raw_index() {
    let spelling = "record.head!.012?.tail.3?";
    let source = LoweredExpression::new(spelling);
    let ExprKind::Try(projection) = source.kind(source.root, SyntaxKind::TryExpr, spelling) else {
        panic!("expected outer try");
    };
    let ExprKind::TupleProjection {
        receiver: tail,
        index,
    } = source.kind(
        *projection,
        SyntaxKind::TupleProjectionExpr,
        "record.head!.012?.tail.3",
    )
    else {
        panic!("expected outer tuple projection");
    };
    assert_eq!(index.as_ref(), "3");
    let ExprKind::Member {
        receiver: inner_try,
        member,
    } = source.kind(*tail, SyntaxKind::MemberExpr, "record.head!.012?.tail")
    else {
        panic!("expected tail member");
    };
    source.identifier(member, "tail", false);

    let ExprKind::Try(inner_projection) =
        source.kind(*inner_try, SyntaxKind::TryExpr, "record.head!.012?")
    else {
        panic!("expected inner try");
    };
    let ExprKind::TupleProjection {
        receiver: head,
        index,
    } = source.kind(
        *inner_projection,
        SyntaxKind::TupleProjectionExpr,
        "record.head!.012",
    )
    else {
        panic!("expected inner tuple projection");
    };
    assert_eq!(index.as_ref(), "012");
    let ExprKind::Member { receiver, member } =
        source.kind(*head, SyntaxKind::MemberExpr, "record.head!")
    else {
        panic!("expected bang member");
    };
    source.identifier(member, "head", true);
    source.name(*receiver, "record", false);
}

#[test]
fn references_preserve_mutability_and_postfix_operand_attachment() {
    for (prefix, expected) in [("&", Mutability::Immutable), ("&mut ", Mutability::Mutable)] {
        let spelling = format!("{prefix}object.payload?");
        let source = LoweredExpression::new(&spelling);
        let ExprKind::Reference { mutability, value } =
            source.kind(source.root, SyntaxKind::RefExpr, &spelling)
        else {
            panic!("expected reference");
        };
        assert_eq!(*mutability, expected);
        let ExprKind::Try(member) = source.kind(*value, SyntaxKind::TryExpr, "object.payload?")
        else {
            panic!("expected try inside reference");
        };
        let ExprKind::Member { receiver, member } =
            source.kind(*member, SyntaxKind::MemberExpr, "object.payload")
        else {
            panic!("expected member inside try");
        };
        source.identifier(member, "payload", false);
        source.name(*receiver, "object", false);
    }
}

#[test]
fn nested_unary_and_postfix_operations_keep_written_attachment() {
    let spelling = "-(*pointers.7?)";
    let source = LoweredExpression::new(spelling);
    let grouped = source.unary(source.root, spelling, UnaryOperator::Negate);
    let dereference = source.group(grouped, "(*pointers.7?)");
    let tried = source.unary(dereference, "*pointers.7?", UnaryOperator::Dereference);
    let ExprKind::Try(projection) = source.kind(tried, SyntaxKind::TryExpr, "pointers.7?") else {
        panic!("expected try operand");
    };
    let ExprKind::TupleProjection { receiver, index } =
        source.kind(*projection, SyntaxKind::TupleProjectionExpr, "pointers.7")
    else {
        panic!("expected projected operand");
    };
    assert_eq!(index.as_ref(), "7");
    source.name(*receiver, "pointers", false);
}

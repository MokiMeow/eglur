use eglur_hir::{ExprId, ExprKind, LiteralKind};
use eglur_syntax::SyntaxKind;

use super::LoweredExpression;

#[test]
fn expression_only_literal_families_preserve_raw_spelling_and_origins() {
    for (spelling, kind, token) in [
        (
            r#"f"line\n{value}""#,
            LiteralKind::FormatString,
            SyntaxKind::FormatString,
        ),
        ("'batch", LiteralKind::AxisName, SyntaxKind::AxisName),
        ("#003", LiteralKind::AxisIndex, SyntaxKind::AxisIndex),
    ] {
        let source = LoweredExpression::new(spelling);
        source.literal(source.root, spelling, kind, token);
    }
}

#[test]
fn name_expressions_preserve_the_effect_marker_separately_from_text() {
    for bang in [false, true] {
        let spelling = if bang { "update!" } else { "update" };
        let source = LoweredExpression::new(spelling);
        source.name(source.root, "update", bang);
    }
}

#[test]
fn resource_expressions_preserve_source_names_and_explicit_move() {
    let own = LoweredExpression::new("own resource");
    let ExprKind::Own(name) = own.kind(own.root, SyntaxKind::OwnExpr, "own resource") else {
        panic!("expected own expression");
    };
    own.identifier(name, "resource", false);

    let moved = LoweredExpression::new("move transferred");
    let ExprKind::Move(name) = moved.kind(moved.root, SyntaxKind::MoveExpr, "move transferred")
    else {
        panic!("expected move expression");
    };
    moved.identifier(name, "transferred", false);

    for (spelling, expected) in [
        ("drop_own disposable", false),
        ("drop_own move disposable", true),
    ] {
        let source = LoweredExpression::new(spelling);
        let ExprKind::DropOwn {
            explicit_move,
            name,
        } = source.kind(source.root, SyntaxKind::DropOwnExpr, spelling)
        else {
            panic!("expected drop_own expression");
        };
        assert_eq!(*explicit_move, expected);
        source.identifier(name, "disposable", false);
    }
}

fn wrapped_block(
    source: &LoweredExpression,
    id: ExprId,
    syntax: SyntaxKind,
    spelling: &str,
) -> ExprId {
    match (syntax, source.kind(id, syntax, spelling)) {
        (SyntaxKind::UnsafeExpr, ExprKind::Unsafe(block))
        | (SyntaxKind::ResultExpr, ExprKind::Result(block))
        | (SyntaxKind::CheckedExpr, ExprKind::Checked(block)) => *block,
        _ => panic!("expected {syntax:?} wrapper: {spelling}"),
    }
}

#[test]
fn wrappers_preserve_their_own_empty_or_populated_block() {
    for (keyword, syntax) in [
        ("unsafe", SyntaxKind::UnsafeExpr),
        ("result", SyntaxKind::ResultExpr),
        ("checked", SyntaxKind::CheckedExpr),
    ] {
        let spelling = format!("{keyword} {{}}");
        let source = LoweredExpression::new(&spelling);
        let block = wrapped_block(&source, source.root, syntax, &spelling);
        let ExprKind::Block { statements } = source.kind(block, SyntaxKind::BlockExpr, "{}") else {
            panic!("expected empty wrapper block");
        };
        assert!(statements.is_empty());

        let spelling = format!("{keyword} {{ yield marker }}");
        let source = LoweredExpression::new(&spelling);
        let block = wrapped_block(&source, source.root, syntax, &spelling);
        let value = source.yielded(block, "{ yield marker }", "yield marker");
        source.name(value, "marker", false);
    }
}

#[test]
fn nested_wrappers_retain_distinct_block_and_yield_edges() {
    let checked = "checked { yield seed }";
    let result_block = format!("{{ yield {checked} }}");
    let result = format!("result {result_block}");
    let unsafe_block = format!("{{ yield {result} }}");
    let spelling = format!("unsafe {unsafe_block}");
    let source = LoweredExpression::new(&spelling);

    let outer = wrapped_block(&source, source.root, SyntaxKind::UnsafeExpr, &spelling);
    let result_id = source.yielded(outer, &unsafe_block, &format!("yield {result}"));
    let middle = wrapped_block(&source, result_id, SyntaxKind::ResultExpr, &result);
    let checked_id = source.yielded(middle, &result_block, &format!("yield {checked}"));
    let inner = wrapped_block(&source, checked_id, SyntaxKind::CheckedExpr, checked);
    let value = source.yielded(inner, "{ yield seed }", "yield seed");
    source.name(value, "seed", false);
}

use eglur_hir::{ExprKind, PatternKind};
use eglur_syntax::SyntaxKind;

use super::LoweredBody;

#[test]
fn if_branches_preserve_roles_and_optional_presence() {
    for (tail, has_else, has_unknown) in [
        ("", false, false),
        (" else { yield 22 }", true, false),
        (" else { yield 22 } unknown { yield 33 }", true, true),
    ] {
        let expression = format!("if condition {{ yield 11 }}{tail}");
        let text = format!("fn sample() -> i64 = {expression}\n");
        let source = LoweredBody::new(&text, "sample");
        let root = source.expression(source.root);
        let ExprKind::If(branches) = root.kind() else {
            panic!("expected an if expression");
        };
        source.written(root.origin(), SyntaxKind::IfExpr, &expression);
        source.name(branches.condition, "condition");
        source.integer(source.yielded(branches.then_branch), "11");
        source.written(
            source.expression(branches.then_branch).origin(),
            SyntaxKind::BlockExpr,
            "{ yield 11 }",
        );
        assert_eq!(branches.else_branch.is_some(), has_else);
        assert_eq!(branches.unknown_branch.is_some(), has_unknown);
        for (branch, value) in [
            (branches.else_branch, "22"),
            (branches.unknown_branch, "33"),
        ] {
            if let Some(branch) = branch {
                source.integer(source.yielded(branch), value);
                source.written(
                    source.expression(branch).origin(),
                    SyntaxKind::BlockExpr,
                    &format!("{{ yield {value} }}"),
                );
            }
        }
    }
}

#[test]
fn else_if_keeps_its_own_condition_else_and_unknown_branches() {
    let inner = "if inner_condition { yield 102 } else { yield 103 } unknown { yield 104 }";
    let expression = format!("if outer_condition {{ yield 101 }} else {inner}");
    let text = format!("fn sample() -> i64 = {expression}\n");
    let source = LoweredBody::new(&text, "sample");
    let root = source.expression(source.root);
    source.written(root.origin(), SyntaxKind::IfExpr, &expression);
    let ExprKind::If(outer) = root.kind() else {
        panic!("expected outer if");
    };
    source.name(outer.condition, "outer_condition");
    assert!(outer.unknown_branch.is_none());

    let nested = source.expression(outer.else_branch.unwrap());
    source.written(nested.origin(), SyntaxKind::IfExpr, inner);
    let ExprKind::If(inner) = nested.kind() else {
        panic!("else-if must remain an if, not a synthetic block");
    };
    source.name(inner.condition, "inner_condition");
    for (branch, value) in [
        (outer.then_branch, "101"),
        (inner.then_branch, "102"),
        (inner.else_branch.unwrap(), "103"),
        (inner.unknown_branch.unwrap(), "104"),
    ] {
        source.integer(source.yielded(branch), value);
        source.written(
            source.expression(branch).origin(),
            SyntaxKind::BlockExpr,
            &format!("{{ yield {value} }}"),
        );
    }
}

#[test]
fn nested_matches_preserve_arm_guard_value_and_scrutinee_roles() {
    let nested = concat!(
        "match nested_input {\n",
        "nested_binding if nested_guard => 111\n",
        "_ => 222\n",
        "}",
    );
    let first_arm = format!("first_binding if first_guard => {nested}");
    let expression = format!(
        "match outer_input {{\n{first_arm}\nsecond_binding => 333\nlast_binding if last_guard => 444\n}}"
    );
    let text = format!("fn sample() -> i64 = {expression}\n");
    let source = LoweredBody::new(&text, "sample");
    let root = source.expression(source.root);
    source.written(root.origin(), SyntaxKind::MatchExpr, &expression);
    let ExprKind::Match(outer) = root.kind() else {
        panic!("expected outer match");
    };
    source.name(outer.scrutinee, "outer_input");
    assert_eq!(outer.arms.len(), 3);
    for (arm, (binding, guard, spelling)) in outer.arms.iter().zip([
        ("first_binding", Some("first_guard"), first_arm.as_str()),
        ("second_binding", None, "second_binding => 333"),
        (
            "last_binding",
            Some("last_guard"),
            "last_binding if last_guard => 444",
        ),
    ]) {
        source.binding(arm.pattern, binding);
        source.written(arm.origin, SyntaxKind::MatchArm, spelling);
        assert_eq!(arm.guard.is_some(), guard.is_some());
        if let (Some(actual), Some(expected)) = (&arm.guard, guard) {
            source.name(actual.condition, expected);
            source.written(actual.origin, SyntaxKind::MatchGuard, expected);
        }
    }
    source.integer(outer.arms[1].value, "333");
    source.integer(outer.arms[2].value, "444");

    let inner = source.expression(outer.arms[0].value);
    source.written(inner.origin(), SyntaxKind::MatchExpr, nested);
    let ExprKind::Match(inner) = inner.kind() else {
        panic!("first arm must retain its nested match");
    };
    source.name(inner.scrutinee, "nested_input");
    assert_eq!(inner.arms.len(), 2);
    source.binding(inner.arms[0].pattern, "nested_binding");
    source.written(
        inner.arms[0].origin,
        SyntaxKind::MatchArm,
        "nested_binding if nested_guard => 111",
    );
    let guard = inner.arms[0].guard.as_ref().unwrap();
    source.name(guard.condition, "nested_guard");
    source.written(guard.origin, SyntaxKind::MatchGuard, "nested_guard");
    source.integer(inner.arms[0].value, "111");
    source.integer(inner.arms[1].value, "222");
    source.written(inner.arms[1].origin, SyntaxKind::MatchArm, "_ => 222");
    assert!(inner.arms[1].guard.is_none());
    let wildcard = source
        .hir
        .body(source.root.body())
        .pattern(inner.arms[1].pattern)
        .unwrap();
    assert!(matches!(wildcard.kind(), PatternKind::Wildcard));
    source.written(wildcard.origin(), SyntaxKind::WildcardPattern, "_");
}

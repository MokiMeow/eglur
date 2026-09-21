use eglur_hir::{LiteralKind, PatternKind, PatternPayload};
use eglur_syntax::SyntaxKind;
use eglur_syntax::ast::{AstNode, Pattern as SyntaxPattern};

use super::{LoweredPattern, child};

#[test]
fn or_alternatives_preserve_alias_group_and_nested_attachment() {
    let left = "nested @ Choice::Left(first)";
    let right = "Option::Right(second)";
    let inner = format!("{left} | {right}");
    let grouped = format!("({inner})");
    let first = format!("whole @ {grouped}");
    let second = "last @ Final::Third(third)";
    let spelling = format!("{first} | {second}");
    let source = LoweredPattern::new(&spelling);
    let PatternKind::Or(alternatives) = source.kind(source.root, SyntaxKind::OrPattern, &spelling)
    else {
        panic!("expected outer or-pattern");
    };
    assert_eq!(alternatives.len(), 2);
    let group = source.alias(alternatives[0], &first, "whole");
    let inner_id = source.group(group, &grouped);
    let PatternKind::Or(nested) = source.kind(inner_id, SyntaxKind::OrPattern, &inner) else {
        panic!("expected grouped or-pattern");
    };
    assert_eq!(nested.len(), 2);

    let left_id = source.alias(nested[0], left, "nested");
    let last_id = source.alias(alternatives[1], second, "last");
    for (id, spelling, path, binding) in [
        (left_id, "Choice::Left(first)", "Choice::Left", "first"),
        (nested[1], right, "Option::Right", "second"),
        (last_id, "Final::Third(third)", "Final::Third", "third"),
    ] {
        let PatternPayload::Positional(elements) =
            source.variant(id, SyntaxKind::VariantPattern, spelling, path)
        else {
            panic!("expected positional variant");
        };
        assert_eq!(elements.len(), 1);
        source.binding(child(&elements[0]), binding);
    }
}

#[test]
fn literal_patterns_preserve_token_kind_spelling_and_exact_origin() {
    for (spelling, kind, token) in [
        ("1_023", LiteralKind::Integer, SyntaxKind::Integer),
        ("2.5e-1", LiteralKind::Float, SyntaxKind::Float),
        ("true", LiteralKind::Bool, SyntaxKind::TrueKw),
        ("false", LiteralKind::Bool, SyntaxKind::FalseKw),
        (r#""line\nnext""#, LiteralKind::String, SyntaxKind::String),
    ] {
        let source = LoweredPattern::new(spelling);
        source.literal(source.root, spelling, kind, token);
    }
}

#[test]
fn unit_group_and_singleton_tuple_remain_distinct() {
    let unit = LoweredPattern::new("()");
    assert!(matches!(
        unit.kind(unit.root, SyntaxKind::UnitPattern, "()"),
        PatternKind::Unit
    ));

    let group = LoweredPattern::new("(grouped)");
    group.binding(group.group(group.root, "(grouped)"), "grouped");

    let tuple = LoweredPattern::new("(single,)");
    let elements = tuple.tuple(tuple.root, "(single,)");
    assert_eq!(elements.len(), 1);
    tuple.binding(child(&elements[0]), "single");

    let nested = LoweredPattern::new("(())");
    let inner = nested.group(nested.root, "(())");
    assert!(matches!(
        nested.kind(inner, SyntaxKind::UnitPattern, "()"),
        PatternKind::Unit
    ));
}

#[test]
fn pattern_facade_preserves_source_identity_without_claiming_payload_containers() {
    let source = LoweredPattern::new(
        "outer @ (Owner::Case(41, (_, ()), { nested: leaf, .. }, ..) | Alt::None)",
    );
    let mut pending = vec![source.parsed.syntax()];
    let mut pattern_kinds = Vec::new();
    let mut containers = Vec::new();
    while let Some(node) = pending.pop() {
        let kind = node.kind();
        if matches!(
            kind,
            SyntaxKind::PatternList | SyntaxKind::FieldPatternList | SyntaxKind::FieldPattern
        ) {
            assert!(SyntaxPattern::cast(node.clone()).is_none());
            containers.push(kind);
        } else if let Some(pattern) = SyntaxPattern::cast(node.clone()) {
            assert_eq!(pattern.syntax().kind(), kind);
            assert_eq!(pattern.syntax().span(), node.span());
            assert_eq!(pattern.syntax().span().file(), source.hir.file());
            assert_eq!(pattern.syntax().text(), node.text());
            let children: Vec<_> = node
                .children()
                .map(|child| (child.kind(), child.span()))
                .collect();
            assert_eq!(
                pattern
                    .syntax()
                    .children()
                    .map(|child| (child.kind(), child.span()))
                    .collect::<Vec<_>>(),
                children
            );
            pattern_kinds.push(kind);
        }
        pending.extend(node.children());
    }

    for kind in [
        SyntaxKind::Pattern,
        SyntaxKind::OrPattern,
        SyntaxKind::AliasPattern,
        SyntaxKind::BindingPattern,
        SyntaxKind::WildcardPattern,
        SyntaxKind::GroupPattern,
        SyntaxKind::LiteralPattern,
        SyntaxKind::UnitPattern,
        SyntaxKind::VariantPattern,
        SyntaxKind::TuplePattern,
        SyntaxKind::StructPattern,
    ] {
        assert!(
            pattern_kinds.contains(&kind),
            "missing exercised pattern kind {kind:?}"
        );
    }
    for kind in [
        SyntaxKind::PatternList,
        SyntaxKind::FieldPatternList,
        SyntaxKind::FieldPattern,
    ] {
        assert!(
            containers.contains(&kind),
            "missing exercised container kind {kind:?}"
        );
    }
}

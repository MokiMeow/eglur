use eglur_hir::{FieldPattern, LiteralKind, PatternElement, PatternKind, PatternPayload};
use eglur_syntax::SyntaxKind;

use super::{LoweredPattern, child};

#[test]
fn variant_payload_presence_and_empty_forms_are_preserved() {
    for path in ["Owner::Empty", "Some"] {
        for suffix in ["", "()", "{}"] {
            let spelling = format!("{path}{suffix}");
            let source = LoweredPattern::new(&spelling);
            let syntax = match (path, suffix) {
                ("Some", "") => SyntaxKind::BindingPattern,
                ("Some", "{}") => SyntaxKind::StructPattern,
                _ => SyntaxKind::VariantPattern,
            };
            let payload = source.variant(source.root, syntax, &spelling, path);
            match (suffix, payload) {
                ("", PatternPayload::Absent) => {}
                ("()", PatternPayload::Positional(elements)) => assert!(elements.is_empty()),
                ("{}", PatternPayload::Named(fields)) => assert!(fields.is_empty()),
                _ => panic!("payload presence or shape changed: {spelling}: {payload:?}"),
            }
        }
    }
}

#[test]
fn positional_payloads_preserve_heterogeneous_prefix_and_tail_rest() {
    let payload = "(29, alias @ (name), (_, \"last\"), ..)";
    for head in ["", "Owner::Packed"] {
        let spelling = format!("{head}{payload}");
        let source = LoweredPattern::new(&spelling);
        let elements = if head.is_empty() {
            source.tuple(source.root, &spelling)
        } else {
            let PatternPayload::Positional(elements) =
                source.variant(source.root, SyntaxKind::VariantPattern, &spelling, head)
            else {
                panic!("expected positional payload");
            };
            elements
        };
        assert_eq!(elements.len(), 4);
        source.literal(
            child(&elements[0]),
            "29",
            LiteralKind::Integer,
            SyntaxKind::Integer,
        );
        let group = source.alias(child(&elements[1]), "alias @ (name)", "alias");
        source.binding(source.group(group, "(name)"), "name");
        let tuple = source.tuple(child(&elements[2]), "(_, \"last\")");
        assert_eq!(tuple.len(), 2);
        assert!(matches!(
            source.kind(child(&tuple[0]), SyntaxKind::WildcardPattern, "_"),
            PatternKind::Wildcard
        ));
        source.literal(
            child(&tuple[1]),
            "\"last\"",
            LiteralKind::String,
            SyntaxKind::String,
        );

        let PatternElement::Rest(origin) = elements[3] else {
            panic!("tail rest was dropped or relocated");
        };
        source.written(origin, SyntaxKind::Range, "..");
    }
}

#[test]
fn rest_only_tuple_and_variant_payloads_are_not_empty() {
    for head in ["", "Owner::Any"] {
        let spelling = format!("{head}(..)");
        let source = LoweredPattern::new(&spelling);
        let elements = if head.is_empty() {
            source.tuple(source.root, &spelling)
        } else {
            let PatternPayload::Positional(elements) =
                source.variant(source.root, SyntaxKind::VariantPattern, &spelling, head)
            else {
                panic!("expected positional payload");
            };
            elements
        };
        let [PatternElement::Rest(origin)] = elements else {
            panic!("expected exactly one positional rest");
        };
        source.written(*origin, SyntaxKind::Range, "..");
    }
}

#[test]
fn named_payloads_preserve_shorthand_and_heterogeneous_field_children() {
    let fields_text = "{ short, pair: (37, kept), nested: Child::Case { item: bound }, .. }";
    for head in ["", "Record ", "Owner::Named ", "Some "] {
        let spelling = format!("{head}{fields_text}");
        let source = LoweredPattern::new(&spelling);
        let fields = match head {
            "" | "Record " => {
                let PatternKind::Struct { path, fields } =
                    source.kind(source.root, SyntaxKind::StructPattern, &spelling)
                else {
                    panic!("expected struct pattern");
                };
                match (head, path) {
                    ("", None) => {}
                    ("Record ", Some(path)) => source.path(path, "Record"),
                    _ => panic!("struct head presence changed"),
                }
                fields
            }
            _ => {
                let syntax = if head == "Some " {
                    SyntaxKind::StructPattern
                } else {
                    SyntaxKind::VariantPattern
                };
                let PatternPayload::Named(fields) =
                    source.variant(source.root, syntax, &spelling, head.trim())
                else {
                    panic!("expected named variant payload");
                };
                fields
            }
        };
        assert_eq!(fields.len(), 4);
        assert!(source.field(&fields[0], "short", "short").is_none());
        let pair = source
            .field(&fields[1], "pair", "pair: (37, kept)")
            .unwrap();
        let elements = source.tuple(pair, "(37, kept)");
        assert_eq!(elements.len(), 2);
        source.literal(
            child(&elements[0]),
            "37",
            LiteralKind::Integer,
            SyntaxKind::Integer,
        );
        source.binding(child(&elements[1]), "kept");

        let nested = source
            .field(&fields[2], "nested", "nested: Child::Case { item: bound }")
            .unwrap();
        let PatternPayload::Named(inner) = source.variant(
            nested,
            SyntaxKind::VariantPattern,
            "Child::Case { item: bound }",
            "Child::Case",
        ) else {
            panic!("expected nested named variant");
        };
        assert_eq!(inner.len(), 1);
        source.binding(
            source.field(&inner[0], "item", "item: bound").unwrap(),
            "bound",
        );

        let FieldPattern::Rest(origin) = fields[3] else {
            panic!("expected outer named rest after fields");
        };
        source.written(origin, SyntaxKind::Range, "..");
    }
}

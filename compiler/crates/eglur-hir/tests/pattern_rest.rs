use eglur_hir::{
    Body, ExprKind, FieldPattern, FunctionBody, HirFile, InvalidCategory, ItemKind, OriginId,
    PatternId, PatternKind, PatternPayload, SourceOrigin, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxDiagnosticKind, SyntaxKind, parse};

fn parse_text(text: &str) -> Parse {
    let source = SourceFile::new(FileId::new(7), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("test source should reach the parser");
    };
    assert_eq!(parsed.syntax().text(), text);
    parsed
}

fn match_pattern(hir: &HirFile) -> (&Body, PatternId) {
    let [root] = hir.roots() else {
        panic!("expected one declaration");
    };
    let ItemKind::Function(function) = hir.item(*root).kind() else {
        panic!("expected a function");
    };
    let FunctionBody::Expression(expression) = function.body_form else {
        panic!("expected an expression body");
    };
    let body = hir.body(function.body);
    let ExprKind::Match(matched) = body.expression(expression).unwrap().kind() else {
        panic!("expected a match expression");
    };
    let [arm] = matched.arms.as_ref() else {
        panic!("expected one arm");
    };
    assert_eq!(arm.pattern.body(), body.id());
    (body, arm.pattern)
}

fn named_fields(body: &Body, pattern: PatternId) -> &[FieldPattern] {
    match body.pattern(pattern).unwrap().kind() {
        PatternKind::Struct { fields, .. }
        | PatternKind::Variant {
            payload: PatternPayload::Named(fields),
            ..
        } => fields,
        other => panic!("expected named fields, got {other:?}"),
    }
}

fn written(hir: &HirFile, origin: OriginId, start: usize, spelling: &str) {
    let SourceOrigin::Written(span) = hir.origin(origin) else {
        panic!("pattern origin should be written");
    };
    assert_eq!(span.file(), FileId::new(7));
    assert_eq!(span.range().start().as_usize(), start);
    assert_eq!(span.range().end().as_usize(), start + spelling.len());
}

fn written_node(hir: &HirFile, parsed: &Parse, origin: OriginId, kind: SyntaxKind, spelling: &str) {
    let mut pending = vec![parsed.syntax()];
    let mut spans = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == kind && node.text().trim() == spelling {
            spans.push(node.span());
        }
        pending.extend(node.children());
    }
    assert_eq!(spans.len(), 1, "unique {kind:?}: {spelling}");
    assert_eq!(hir.origin(origin), SourceOrigin::Written(spans[0]));
}

#[test]
fn named_rest_preserves_order_shorthand_explicit_fields_and_origins() {
    for head in ["", "Record ", "Owner::Case "] {
        for payload in [
            "..",
            ".., first, last: renamed",
            "first, .., last: renamed",
            "first, last: renamed, ..",
        ] {
            let pattern = format!("{head}{{ {payload} }}");
            let text =
                format!("fn inspect(input: Input) -> Unit = match input {{ {pattern} => () }}\n");
            let parsed = parse_text(&text);
            assert!(parsed.is_success(), "{pattern}: {:?}", parsed.diagnostics());
            let hir = lower(&parsed);
            assert!(hir.invalid_occurrences().is_empty());
            let (body, pattern_id) = match_pattern(&hir);
            let value = body.pattern(pattern_id).unwrap();
            let kind = if head == "Owner::Case " {
                SyntaxKind::VariantPattern
            } else {
                SyntaxKind::StructPattern
            };
            written_node(&hir, &parsed, value.origin(), kind, &pattern);
            match (head, value.kind()) {
                ("", PatternKind::Struct { path: None, .. }) => {}
                (
                    "Record ",
                    PatternKind::Struct {
                        path: Some(path), ..
                    },
                ) => {
                    assert_eq!(path.text().as_deref(), Some("Record"));
                }
                ("Owner::Case ", PatternKind::Variant { path, .. }) => {
                    assert_eq!(path.text().as_deref(), Some("Owner::Case"));
                }
                _ => panic!("wrong named-pattern head: {value:?}"),
            }

            let fields = named_fields(body, pattern_id);
            let expected: Vec<_> = payload.split(", ").collect();
            assert_eq!(fields.len(), expected.len());
            for (field, spelling) in fields.iter().zip(expected) {
                let start = text.find(spelling).unwrap();
                match field {
                    FieldPattern::Rest(origin) => {
                        assert_eq!(spelling, "..");
                        written(&hir, *origin, start, "..");
                    }
                    FieldPattern::Field {
                        name,
                        pattern,
                        origin,
                    } => {
                        let (key, binding) = spelling
                            .split_once(": ")
                            .map_or((spelling, None), |(key, value)| (key, Some(value)));
                        assert_eq!(name.text(), Some(key));
                        written(&hir, name.origin(), start, key);
                        written_node(&hir, &parsed, *origin, SyntaxKind::FieldPattern, spelling);
                        match (pattern, binding) {
                            (None, None) => {}
                            (Some(id), Some(binding)) => {
                                assert_eq!(id.body(), body.id());
                                let value = body.pattern(*id).unwrap();
                                let PatternKind::Binding(name) = value.kind() else {
                                    panic!("expected explicit binding");
                                };
                                assert_eq!(name.text(), Some(binding));
                                let start = start + key.len() + 2;
                                written_node(
                                    &hir,
                                    &parsed,
                                    value.origin(),
                                    SyntaxKind::BindingPattern,
                                    binding,
                                );
                                written(&hir, name.origin(), start, binding);
                            }
                            _ => panic!("shorthand/explicit field changed: {spelling}"),
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn nested_named_rest_stays_in_its_own_payload() {
    let text = "fn inspect(input: Input) -> Unit = match input { \
                Outer { .., child: Inner { .., leaf }, sibling: Other { value, .. }, tail } \
                => () }\n";
    let parsed = parse_text(text);
    assert!(parsed.is_success(), "{:?}", parsed.diagnostics());
    let hir = lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    let (body, pattern) = match_pattern(&hir);
    let [
        FieldPattern::Rest(outer),
        FieldPattern::Field {
            name: child,
            pattern: Some(inner),
            ..
        },
        FieldPattern::Field {
            name: sibling,
            pattern: Some(other),
            ..
        },
        FieldPattern::Field {
            name: tail,
            pattern: None,
            ..
        },
    ] = named_fields(body, pattern)
    else {
        panic!("outer field order or child links changed");
    };
    assert_eq!(child.text(), Some("child"));
    assert_eq!(sibling.text(), Some("sibling"));
    assert_eq!(tail.text(), Some("tail"));
    assert_eq!(inner.body(), body.id());
    assert_eq!(other.body(), body.id());

    let [
        FieldPattern::Rest(inner_rest),
        FieldPattern::Field {
            name: leaf,
            pattern: None,
            ..
        },
    ] = named_fields(body, *inner)
    else {
        panic!("inner rest must precede its own field");
    };
    let [
        FieldPattern::Field {
            name: value,
            pattern: None,
            ..
        },
        FieldPattern::Rest(other_rest),
    ] = named_fields(body, *other)
    else {
        panic!("sibling rest must follow its own field");
    };
    assert_eq!(leaf.text(), Some("leaf"));
    assert_eq!(value.text(), Some("value"));
    let rest_starts: Vec<_> = text.match_indices("..").map(|(start, _)| start).collect();
    assert_eq!(rest_starts.len(), 3);
    for (origin, start) in [outer, inner_rest, other_rest].into_iter().zip(rest_starts) {
        written(&hir, *origin, start, "..");
    }
}

#[test]
fn duplicate_rest_retains_its_cause_and_following_declaration() {
    for pattern in [
        "{ .., first, .. }",
        "Record { first, .., last: renamed, .. }",
        "Owner::Case { .., first, .. }",
        "Outer { .., child: Inner { .., leaf, .. } }",
    ] {
        let text = format!(
            "fn inspect(input: Input) -> Unit = match input {{ {pattern} => () }}\n\
             const Following: Unit = ()\n"
        );
        let parsed = parse_text(&text);
        assert_eq!(parsed.diagnostics().len(), 1, "{pattern}");
        let diagnostic = parsed.diagnostics()[0];
        assert_eq!(
            diagnostic.kind(),
            SyntaxDiagnosticKind::ParseInvalidRestPosition
        );
        let hir = lower(&parsed);
        let regions: Vec<_> = parsed
            .syntax()
            .children()
            .filter(|node| node.kind() == SyntaxKind::ErrorNode)
            .collect();
        assert!(!regions.is_empty());
        assert_eq!(hir.roots().len(), regions.len() + 1);
        let causes: Vec<_> = hir.syntax_causes().collect();
        let [(cause, span)] = causes.as_slice() else {
            panic!("expected one syntax cause");
        };
        assert_eq!(*span, diagnostic.span());
        for (&root, region) in hir.roots().iter().zip(&regions) {
            let item = hir.item(root);
            assert!(matches!(item.kind(), ItemKind::Invalid));
            assert_eq!(
                hir.origin(item.origin()),
                SourceOrigin::Written(region.span())
            );
        }
        assert_eq!(hir.invalid_occurrences().len(), regions.len() + 1);
        for occurrence in hir.invalid_occurrences() {
            assert_eq!(occurrence.syntax_causes(), &[*cause]);
            if occurrence.category() == InvalidCategory::ReportedSyntax {
                assert_eq!(
                    hir.origin(occurrence.origin()),
                    SourceOrigin::Written(*span)
                );
            } else {
                assert_eq!(occurrence.category(), InvalidCategory::Item);
            }
        }

        let sentinel = hir.item(*hir.roots().last().unwrap());
        let ItemKind::Const(constant) = sentinel.kind() else {
            panic!("following declaration was not recovered");
        };
        assert_eq!(constant.name.text(), Some("Following"));
        let value = hir.body(constant.body).expression(constant.value).unwrap();
        assert!(matches!(value.kind(), ExprKind::Unit));
    }
}

use eglur_hir::{
    ExprKind, FieldPattern, HirFile, InvalidCategory, ItemKind, Mutability, OriginId,
    PatternElement, PatternKind, PatternPayload, PointerMutability, TypeRefKind, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, ParseResource, SyntaxDiagnosticKind, parse};

const TYPE_FORMS: &[(&str, &str)] = &[
    ("& ", ""),
    ("&mut ", ""),
    ("*const ", ""),
    ("*mut ", ""),
    ("Seq[", "]"),
    ("(", ")"),
    ("(", ", i64)"),
    ("Fn(", ") -> i64"),
    ("Fn() -> ", ""),
];

const PATTERN_FORMS: &[(&str, &str)] = &[
    ("(", ")"),
    ("(", ", _)"),
    ("a @ ", ""),
    ("Some(", ")"),
    ("Point{x: ", "}"),
];

fn nested(prefix: &str, leaf: &str, suffix: &str, depth: usize) -> String {
    format!("{}{leaf}{}", prefix.repeat(depth), suffix.repeat(depth))
}

fn parse_text(text: &str) -> Parse {
    let source = SourceFile::new(FileId::new(10), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8 source must parse")
    };
    assert_eq!(parsed.syntax().text(), text);
    parsed
}

fn assert_nested_origins(hir: &HirFile, parent: OriginId, child: OriginId) {
    assert!(parent.get() < child.get());
    let parent = hir.origin(parent).source();
    let child = hir.origin(child).source();
    assert_eq!(parent.file(), child.file());
    assert!(parent.range().start() <= child.range().start());
    assert!(child.range().end() <= parent.range().end());
}

#[test]
fn nested_types_preserve_structure_at_the_parser_budget() {
    // The parameter's outer parse_type consumes one of the 256 recursion slots.
    let depth = 255;
    for &(prefix, suffix) in TYPE_FORMS {
        let ty = nested(prefix, "i64", suffix, depth);
        let parsed = parse_text(&format!("fn f(x: {ty}) -> Unit = ()\n"));
        assert!(parsed.is_success(), "{prefix}: {:?}", parsed.diagnostics());
        let hir = lower(&parsed);
        assert!(hir.invalid_occurrences().is_empty());
        let ItemKind::Function(function) = hir.item(hir.roots()[0]).kind() else {
            panic!("expected function")
        };
        let mut id = function.parameters[0].ty.unwrap();

        for _ in 0..depth {
            let ty = hir.type_ref(id);
            let child = match (prefix, suffix, ty.kind()) {
                (
                    "& " | "&mut ",
                    _,
                    TypeRefKind::Reference {
                        mutability,
                        referent,
                    },
                ) => {
                    assert_eq!(
                        *mutability,
                        if prefix == "& " {
                            Mutability::Immutable
                        } else {
                            Mutability::Mutable
                        }
                    );
                    *referent
                }
                (
                    "*const " | "*mut ",
                    _,
                    TypeRefKind::Pointer {
                        mutability,
                        referent,
                    },
                ) => {
                    assert_eq!(
                        *mutability,
                        if prefix == "*const " {
                            PointerMutability::Const
                        } else {
                            PointerMutability::Mutable
                        }
                    );
                    *referent
                }
                ("Seq[", _, TypeRefKind::Path { path, arguments }) => {
                    assert_eq!(path.segments()[0].text(), Some("Seq"));
                    assert_eq!(arguments.len(), 1);
                    arguments[0]
                }
                ("(", ")", TypeRefKind::Group(inner)) => *inner,
                ("(", ", i64)", TypeRefKind::Tuple(elements)) => {
                    assert_eq!(elements.len(), 2);
                    assert_scalar_type(&hir, elements[1]);
                    elements[0]
                }
                ("Fn(", _, TypeRefKind::Function { parameters, result }) => {
                    assert_eq!(parameters.len(), 1);
                    assert_scalar_type(&hir, *result);
                    parameters[0]
                }
                ("Fn() -> ", _, TypeRefKind::Function { parameters, result }) => {
                    assert!(parameters.is_empty());
                    *result
                }
                _ => panic!("unexpected nested type for {prefix}: {:?}", ty.kind()),
            };
            assert!(child.get() < id.get());
            assert_nested_origins(&hir, ty.origin(), hir.type_ref(child).origin());
            id = child;
        }
        assert_scalar_type(&hir, id);
    }
}

fn assert_scalar_type(hir: &HirFile, id: eglur_hir::TypeRefId) {
    let TypeRefKind::Path { path, arguments } = hir.type_ref(id).kind() else {
        panic!("expected scalar type")
    };
    assert_eq!(path.segments()[0].text(), Some("i64"));
    assert!(arguments.is_empty());
}

#[test]
fn nested_patterns_preserve_structure_at_the_parser_budget() {
    // The match expression and outer pattern consume the other two slots.
    let depth = 254;
    for &(prefix, suffix) in PATTERN_FORMS {
        let pattern = nested(prefix, "x", suffix, depth);
        let parsed = parse_text(&format!(
            "fn f(v: i64) -> i64 = match v {{ {pattern} => 0 }}\n"
        ));
        assert!(parsed.is_success(), "{prefix}: {:?}", parsed.diagnostics());
        let hir = lower(&parsed);
        assert!(hir.invalid_occurrences().is_empty());
        let (_, body) = hir.bodies().next().unwrap();
        let mut id = body
            .expressions()
            .find_map(|(_, expr)| match expr.kind() {
                ExprKind::Match(expr) => Some(expr.arms[0].pattern),
                _ => None,
            })
            .unwrap();

        for _ in 0..depth {
            let pattern = body.pattern(id).unwrap();
            let child = match (prefix, suffix, pattern.kind()) {
                ("(", ")", PatternKind::Group(inner)) => *inner,
                ("(", ", _)", PatternKind::Tuple(elements)) => {
                    let [
                        PatternElement::Pattern(inner),
                        PatternElement::Pattern(wildcard),
                    ] = &**elements
                    else {
                        panic!("expected two tuple patterns")
                    };
                    assert!(matches!(
                        body.pattern(*wildcard).unwrap().kind(),
                        PatternKind::Wildcard
                    ));
                    *inner
                }
                ("a @ ", _, PatternKind::Alias { name, pattern }) => {
                    assert_eq!(name.text(), Some("a"));
                    *pattern
                }
                (
                    "Some(",
                    _,
                    PatternKind::Variant {
                        path,
                        payload: PatternPayload::Positional(elements),
                    },
                ) => {
                    assert_eq!(path.segments()[0].text(), Some("Some"));
                    let [PatternElement::Pattern(inner)] = &**elements else {
                        panic!("expected one variant pattern")
                    };
                    *inner
                }
                ("Point{x: ", _, PatternKind::Struct { path, fields }) => {
                    assert_eq!(path.as_ref().unwrap().segments()[0].text(), Some("Point"));
                    let [
                        FieldPattern::Field {
                            name,
                            pattern: Some(inner),
                            ..
                        },
                    ] = &**fields
                    else {
                        panic!("expected one named field pattern")
                    };
                    assert_eq!(name.text(), Some("x"));
                    *inner
                }
                _ => panic!(
                    "unexpected nested pattern for {prefix}: {:?}",
                    pattern.kind()
                ),
            };
            assert_eq!(id.body(), child.body());
            assert!(child.local() < id.local());
            assert_nested_origins(
                &hir,
                pattern.origin(),
                body.pattern(child).unwrap().origin(),
            );
            id = child;
        }
        let PatternKind::Binding(name) = body.pattern(id).unwrap().kind() else {
            panic!("expected terminal binding")
        };
        assert_eq!(name.text(), Some("x"));
    }
}

fn assert_resource_recovery(text: &str) {
    let parsed = parse_text(text);
    assert!(!parsed.is_success());
    let diagnostic = parsed
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.kind() == SyntaxDiagnosticKind::ParseResourceLimitExceeded)
        .expect("nesting must reach a structured parser resource limit");
    let limit = diagnostic.resource_limit().unwrap();
    assert!(matches!(
        limit.resource(),
        ParseResource::RecursiveDepth | ParseResource::DelimiterDepth
    ));
    assert_eq!(limit.limit(), 256);
    assert_eq!(limit.observed_at_least(), 257);

    let hir = lower(&parsed);
    let causes = hir.syntax_causes().collect::<Vec<_>>();
    assert_eq!(causes.len(), parsed.diagnostics().len());
    for ((id, span), diagnostic) in causes.iter().zip(parsed.diagnostics()) {
        assert_eq!(*span, diagnostic.span());
        assert!(hir.invalid_occurrences().iter().any(|occurrence| {
            occurrence.category() == InvalidCategory::ReportedSyntax
                && occurrence.syntax_causes().contains(id)
                && hir.origin(occurrence.origin()).source() == *span
        }));
    }
}

#[test]
fn excessive_type_nesting_preserves_source_and_causal_diagnostics() {
    for depth in [256, 512] {
        for &(prefix, suffix) in TYPE_FORMS {
            let ty = nested(prefix, "i64", suffix, depth);
            assert_resource_recovery(&format!("fn f(x: {ty}) -> Unit = ()\n"));
        }
    }
}

#[test]
fn excessive_pattern_nesting_preserves_source_and_causal_diagnostics() {
    for depth in [255, 512] {
        for &(prefix, suffix) in PATTERN_FORMS {
            let pattern = nested(prefix, "x", suffix, depth);
            assert_resource_recovery(&format!(
                "fn f(v: i64) -> i64 = match v {{ {pattern} => 0 }}\n"
            ));
        }
    }
}

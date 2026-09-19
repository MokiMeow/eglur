use eglur_hir::{ExprKind, HirFile, InvalidCategory, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, SyntaxDiagnosticKind, parse};

fn lower_text(text: &str, success: bool) -> HirFile {
    let source = SourceFile::new(FileId::new(11), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8 source must parse")
    };
    assert_eq!(parsed.syntax().text(), text);
    assert_eq!(parsed.is_success(), success, "{:?}", parsed.diagnostics());
    lower(&parsed)
}

struct NestedCase {
    name: &'static str,
    prefix: &'static str,
    suffix: &'static str,
    depth: usize,
    matches: fn(&ExprKind) -> bool,
}

impl NestedCase {
    fn new(
        name: &'static str,
        prefix: &'static str,
        suffix: &'static str,
        depth: usize,
        matches: fn(&ExprKind) -> bool,
    ) -> Self {
        Self {
            name,
            prefix,
            suffix,
            depth,
            matches,
        }
    }
}

fn nested_cases() -> Vec<NestedCase> {
    let call: fn(&ExprKind) -> bool = |kind| matches!(kind, ExprKind::Call(_));
    vec![
        NestedCase::new("call argument", "f(", ")", 255, call),
        NestedCase::new("named argument", "f(value = ", ")", 255, call),
        NestedCase::new("typed call", "f::[i64](", ")", 255, call),
        NestedCase::new("dtype call", "f[f32](", ")", 255, |kind| {
            matches!(kind, ExprKind::DTypeCall(_))
        }),
        NestedCase::new("catalog call", "Vec[i64].from_seq(", ")", 255, |kind| {
            matches!(kind, ExprKind::CatalogCall(_))
        }),
        NestedCase::new("then callable", "x.then(", ")", 255, call),
        NestedCase::new("then input", "(", ").then(f)", 255, call),
        NestedCase::new("apply input", "(", ", x).apply(f)", 255, call),
        NestedCase::new("tuple", "(", ", x)", 255, |kind| {
            matches!(kind, ExprKind::Tuple(_))
        }),
        NestedCase::new("array", "[", "]", 255, |kind| {
            matches!(kind, ExprKind::Array(_))
        }),
        NestedCase::new("map value", "[x: ", "]", 255, |kind| {
            matches!(kind, ExprKind::Map(_))
        }),
        NestedCase::new("map key", "[", ": x]", 255, |kind| {
            matches!(kind, ExprKind::Map(_))
        }),
        NestedCase::new("struct field", "Box{x: ", "}", 255, |kind| {
            matches!(kind, ExprKind::StructLiteral { .. })
        }),
        NestedCase::new("variant tuple", "Option::Some(", ")", 255, |kind| {
            matches!(kind, ExprKind::VariantConstructor { .. })
        }),
        NestedCase::new("variant field", "Choice::Value{x: ", "}", 255, |kind| {
            matches!(kind, ExprKind::VariantConstructor { .. })
        }),
        NestedCase::new("block yield", "{ yield ", " }", 255, |kind| {
            matches!(kind, ExprKind::Block { .. })
        }),
        NestedCase::new("unsafe", "unsafe { yield ", " }", 255, |kind| {
            matches!(kind, ExprKind::Unsafe(_))
        }),
        NestedCase::new("result", "result { yield ", " }", 255, |kind| {
            matches!(kind, ExprKind::Result(_))
        }),
        NestedCase::new("checked", "checked { yield ", " }", 255, |kind| {
            matches!(kind, ExprKind::Checked(_))
        }),
        NestedCase::new(
            "if then",
            "if true { yield ",
            " } else { yield x }",
            255,
            |kind| matches!(kind, ExprKind::If(_)),
        ),
        NestedCase::new(
            "if else",
            "if true { yield x } else { yield ",
            " }",
            255,
            |kind| matches!(kind, ExprKind::If(_)),
        ),
        NestedCase::new(
            "if condition",
            "if (",
            ") { yield x } else { yield x }",
            127,
            |kind| matches!(kind, ExprKind::If(_)),
        ),
        NestedCase::new("match value", "match x { _ => ", " }", 254, |kind| {
            matches!(kind, ExprKind::Match(_))
        }),
        NestedCase::new("match guard", "match x { _ if ", " => x }", 254, |kind| {
            matches!(kind, ExprKind::Match(_))
        }),
        NestedCase::new("match scrutinee", "match (", ") { _ => x }", 127, |kind| {
            matches!(kind, ExprKind::Match(_))
        }),
        NestedCase::new("lambda tail", "{ || ", " }", 255, |kind| {
            matches!(kind, ExprKind::Lambda(_))
        }),
        NestedCase::new(
            "lambda statement",
            "{ |v| let local = ",
            "\nlocal }",
            254,
            |kind| matches!(kind, ExprKind::Lambda(_)),
        ),
        NestedCase::new(
            "trailing lambda",
            "f() { || ",
            " }",
            255,
            |kind| matches!(kind, ExprKind::Call(call) if call.syntax == eglur_hir::CallSyntax::TrailingArgument),
        ),
        NestedCase::new("single reducer", "r.reduce_by(", ")", 255, |kind| {
            matches!(kind, ExprKind::ReduceByCandidate(_))
        }),
        NestedCase::new("phase reducer", "r.reduce_by((axis, ", "))", 127, |kind| {
            matches!(kind, ExprKind::ReduceByCandidate(_))
        }),
    ]
}

#[test]
fn nested_expression_children_lower_on_the_default_stack() {
    for case in nested_cases() {
        let text = format!(
            "fn f(x: i64) -> i64 = {}x{}\n",
            case.prefix.repeat(case.depth),
            case.suffix.repeat(case.depth)
        );
        eprintln!("{}", case.name);
        let hir = lower_text(&text, true);
        assert!(hir.invalid_occurrences().is_empty(), "{}", case.name);
        let mut count = 0;
        for (body_id, body) in hir.bodies() {
            for (id, expr) in body.expressions() {
                assert_eq!(id.body(), body_id);
                let span = hir.origin(expr.origin()).source();
                assert_eq!(span.file(), hir.file());
                assert!(span.range().end().as_usize() <= text.len());
                if let ExprKind::Lambda(lambda) = expr.kind() {
                    assert_ne!(lambda.body, body_id);
                    let lambda_body = hir.body(lambda.body);
                    for parameter in &lambda.parameters {
                        assert!(lambda_body.pattern(parameter.pattern).is_some());
                    }
                    for statement in &lambda.statements {
                        assert!(lambda_body.statement(*statement).is_some());
                    }
                    if let Some(tail) = lambda.tail {
                        assert!(lambda_body.expression(tail).is_some());
                    }
                }
                count += usize::from((case.matches)(expr.kind()));
            }
        }
        assert_eq!(count, case.depth, "{}", case.name);
    }
}

#[test]
fn nested_statements_keep_initializers_targets_returns_and_loop_bodies() {
    for (prefix, suffix) in [
        ("{ let value = ", "\nyield value }"),
        ("{ x = ", "\nyield x }"),
        ("{ ", " = x\nyield x }"),
        ("{ return ", "\n}"),
        ("{ ", "\nyield x }"),
        ("{ while true { yield ", " }\nyield x }"),
        ("{ for v in x { yield ", " }\nyield x }"),
    ] {
        let depth = 120;
        let text = format!(
            "fn f(x: i64) -> i64 = {}x{}\n",
            prefix.repeat(depth),
            suffix.repeat(depth)
        );
        let hir = lower_text(&text, true);
        assert!(hir.invalid_occurrences().is_empty(), "{prefix}");
        let (_, body) = hir.bodies().next().unwrap();
        let block_count = body
            .expressions()
            .filter(|(_, expr)| matches!(expr.kind(), ExprKind::Block { .. }))
            .count();
        assert_eq!(
            block_count,
            if prefix.contains("while") || prefix.contains("for") {
                depth * 2
            } else {
                depth
            }
        );
        for (id, statement) in body.statements() {
            assert_eq!(id.body(), body.id());
            assert!(
                hir.origin(statement.origin())
                    .source()
                    .range()
                    .end()
                    .as_usize()
                    <= text.len()
            );
        }
    }
}

#[test]
fn nested_children_and_flat_spines_share_one_traversal() {
    let depth = 200;
    let text = format!(
        "fn f(x: i64) -> i64 = {}x{}{}\n",
        "f([(".repeat(60),
        ".member()?".repeat(4000),
        ")])".repeat(60)
    );
    let hir = lower_text(&text, true);
    assert!(hir.invalid_occurrences().is_empty());
    let (_, body) = hir.bodies().next().unwrap();
    assert_eq!(
        body.expressions()
            .filter(|(_, expr)| matches!(expr.kind(), ExprKind::Member { .. }))
            .count(),
        4000
    );

    let text = format!(
        "fn f(x: i64) -> i64 = {}x{}\n",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let hir = lower_text(&text, true);
    let (_, body) = hir.bodies().next().unwrap();
    for (id, expr) in body.expressions() {
        if let ExprKind::Array(elements) = expr.kind() {
            assert_eq!(elements.len(), 1);
            assert!(elements[0].local() < id.local());
            assert_eq!(elements[0].body(), id.body());
        }
    }
}

#[test]
fn nested_recovery_retains_reported_causes_and_following_items() {
    for (prefix, suffix) in [("f(", ")"), ("[", "]"), ("{ yield ", " }")] {
        let text = format!(
            "fn f(x: i64) -> i64 = {}x +{}\nfn next() -> Unit = ()\n",
            prefix.repeat(200),
            suffix.repeat(200)
        );
        let hir = lower_text(&text, false);
        assert_eq!(hir.roots().len(), 2);
        assert!(
            hir.invalid_occurrences()
                .iter()
                .any(
                    |occurrence| occurrence.category() == InvalidCategory::ReportedSyntax
                        && !occurrence.syntax_causes().is_empty()
                )
        );
    }

    let text = format!(
        "fn f(x: i64) -> i64 = {}x{}\n",
        "f(".repeat(512),
        ")".repeat(512)
    );
    let source = SourceFile::new(FileId::new(11), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("expected parse")
    };
    assert_eq!(parsed.syntax().text(), text);
    assert!(parsed.diagnostics().iter().any(|diagnostic| diagnostic.kind() == SyntaxDiagnosticKind::ParseResourceLimitExceeded));
    let hir = lower(&parsed);
    assert_eq!(hir.syntax_causes().len(), parsed.diagnostics().len());
}

fn assert_integer(body: &eglur_hir::Body, id: eglur_hir::ExprId, spelling: &str) {
    let ExprKind::Literal(literal) = body.expression(id).unwrap().kind() else {
        panic!("expected integer literal")
    };
    assert_eq!(literal.kind(), eglur_hir::LiteralKind::Integer);
    assert_eq!(literal.spelling(), spelling);
}

#[test]
fn nested_collections_do_not_consume_their_parents_siblings() {
    let depth = 120;
    for (prefix, suffix) in [
        ("f(first = 1, middle = ", ", last = 3)"),
        ("[1: ", ", 2: 3]"),
        ("Box{left: ", ", right: 3}"),
        ("match 1 { _ if true => ", ", _ => 3 }"),
        ("r.reduce_by((a, ", "), (b, 3))"),
    ] {
        let text = format!(
            "fn f(x: i64) -> i64 = {}x{}\n",
            prefix.repeat(depth),
            suffix.repeat(depth)
        );
        let hir = lower_text(&text, true);
        assert!(hir.invalid_occurrences().is_empty());
        let (_, body) = hir.bodies().next().unwrap();
        let mut count = 0;
        for (_, expr) in body.expressions() {
            match expr.kind() {
                ExprKind::Call(call) => {
                    let [first, middle, last] = &*call.arguments else {
                        panic!("expected three named arguments")
                    };
                    assert_eq!(first.name.as_ref().unwrap().text(), Some("first"));
                    assert_eq!(middle.name.as_ref().unwrap().text(), Some("middle"));
                    assert_eq!(last.name.as_ref().unwrap().text(), Some("last"));
                    assert_integer(body, first.value, "1");
                    assert_integer(body, last.value, "3");
                    assert!(first.value.local() < middle.value.local());
                    assert!(middle.value.local() < last.value.local());
                }
                ExprKind::Map(entries) => {
                    let [first, last] = &**entries else {
                        panic!("expected two entries")
                    };
                    assert_integer(body, first.key, "1");
                    assert_integer(body, last.key, "2");
                    assert_integer(body, last.value, "3");
                    assert!(first.key.local() < first.value.local());
                    assert!(first.value.local() < last.key.local());
                }
                ExprKind::StructLiteral { fields, .. } => {
                    let [left, right] = &**fields else {
                        panic!("expected two fields")
                    };
                    assert_eq!(left.name.text(), Some("left"));
                    assert_eq!(right.name.text(), Some("right"));
                    assert_integer(body, right.value, "3");
                    assert!(left.value.local() < right.value.local());
                }
                ExprKind::Match(expr) => {
                    assert_integer(body, expr.scrutinee, "1");
                    let [first, last] = &*expr.arms else {
                        panic!("expected two arms")
                    };
                    assert!(first.guard.is_some());
                    assert!(last.guard.is_none());
                    assert_integer(body, last.value, "3");
                    assert!(first.guard.as_ref().unwrap().condition.local() < first.value.local());
                    assert!(first.value.local() < last.value.local());
                }
                ExprKind::ReduceByCandidate(candidate) => {
                    let phases = &candidate.ordered_phases.as_ref().unwrap().phases;
                    let [first, last] = &**phases else {
                        panic!("expected two phases")
                    };
                    assert_eq!(first.axes[0].text(), Some("a"));
                    assert_eq!(last.axes[0].text(), Some("b"));
                    assert_integer(body, last.reducer, "3");
                    assert!(first.reducer.local() < last.reducer.local());
                    assert_eq!(candidate.ordinary.as_ref().unwrap().call.arguments.len(), 2);
                }
                _ => continue,
            }
            count += 1;
        }
        assert_eq!(count, depth);
    }
}

use eglur_hir::{
    BinaryOperator, ExprKind, HirFile, InvalidCategory, ItemKind, LiteralKind, SourceOrigin, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, parse};

fn lower_text(text: &str, success: bool) -> HirFile {
    let source = SourceFile::new(FileId::new(7), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8 source must parse")
    };
    assert_eq!(parsed.is_success(), success, "{:?}", parsed.diagnostics());
    lower(&parsed)
}

#[test]
fn deep_binary_spines_preserve_operator_and_child_order_without_a_larger_stack() {
    for (symbol, expected) in [
        ("+", BinaryOperator::Add),
        ("-", BinaryOperator::Subtract),
        ("*", BinaryOperator::Multiply),
        ("/", BinaryOperator::Divide),
        ("/.", BinaryOperator::FractionalDivide),
        ("%", BinaryOperator::Remainder),
        ("&&", BinaryOperator::LogicalAnd),
        ("||", BinaryOperator::LogicalOr),
        ("==", BinaryOperator::Equal),
    ] {
        let text = format!(
            "fn f(a: i64) -> i64 = a{}\n",
            format!(" {symbol} a").repeat(4000)
        );
        let hir = lower_text(&text, true);
        let mut count = 0;
        for (_, body) in hir.bodies() {
            for (id, expr) in body.expressions() {
                if let ExprKind::Binary {
                    operator,
                    left,
                    right,
                } = expr.kind()
                {
                    assert_eq!(*operator, expected);
                    assert!(left.local() < right.local());
                    assert!(right.local() < id.local());
                    let parent = hir.origin(expr.origin()).source().range();
                    for child in [left, right] {
                        let child = body.expression(*child).unwrap();
                        let span = hir.origin(child.origin()).source().range();
                        assert!(parent.start() <= span.start());
                        assert!(span.end() <= parent.end());
                        assert!(expr.origin().get() < child.origin().get());
                    }
                    count += 1;
                }
            }
        }
        assert_eq!(count, 4000);
    }
}

#[test]
fn mixed_group_unary_and_binary_spines_preserve_every_child() {
    let text = format!("fn f(a: i64) -> i64 = a{}\n", " + (-a)".repeat(4000));
    let hir = lower_text(&text, true);
    let mut counts = [0; 3];
    for (_, body) in hir.bodies() {
        for (id, expr) in body.expressions() {
            match expr.kind() {
                ExprKind::Binary { left, right, .. } => {
                    assert!(left.local() < right.local() && right.local() < id.local());
                    counts[0] += 1;
                }
                ExprKind::Unary { operand, .. } => {
                    assert!(operand.local() < id.local());
                    counts[1] += 1;
                }
                ExprKind::Group(operand) => {
                    assert!(operand.local() < id.local());
                    counts[2] += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!(counts, [4000; 3]);
}

#[test]
fn signed_literals_keep_their_spelling_and_written_span_inside_operator_trees() {
    let text = "fn f(a: i64) -> i64 = (-128 + -(128)) * (a - -9223372036854775808)\n";
    let hir = lower_text(text, true);
    let mut literals = Vec::new();
    let mut unary = 0;
    for (_, body) in hir.bodies() {
        for (_, expr) in body.expressions() {
            if let ExprKind::Literal(literal) = expr.kind() {
                assert_eq!(literal.kind(), LiteralKind::Integer);
                let SourceOrigin::Written(span) = hir.origin(literal.origin()) else {
                    panic!("literal must retain written origin")
                };
                let range = span.range();
                assert_eq!(
                    &text[range.start().as_usize()..range.end().as_usize()],
                    literal.spelling()
                );
                literals.push(literal.spelling());
            }
            if matches!(expr.kind(), ExprKind::Unary { .. }) {
                unary += 1;
            }
        }
    }
    assert_eq!(literals, ["-128", "128", "-9223372036854775808"]);
    assert_eq!(unary, 1);
}

#[test]
fn deep_malformed_operator_trees_retain_recovery_and_following_declarations() {
    let text = format!(
        "fn f(a: i64) -> i64 = a{} +\nfn next() -> Unit = ()\n",
        " + a".repeat(4000)
    );
    let hir = lower_text(&text, false);
    assert_eq!(hir.roots().len(), 2);
    assert!(!hir.invalid_occurrences().is_empty());
    assert!(hir.invalid_occurrences().iter().any(|occurrence| {
        occurrence.category() == InvalidCategory::ReportedSyntax
            && !occurrence.syntax_causes().is_empty()
    }));
    assert!(matches!(hir.item(hir.roots()[0]).kind(), ItemKind::Invalid));
    let ItemKind::Function(next) = hir.item(hir.roots()[1]).kind() else {
        panic!("following function must survive recovery")
    };
    assert_eq!(next.name.text(), Some("next"));
    assert!(
        hir.body(next.body)
            .expressions()
            .any(|(_, expr)| matches!(expr.kind(), ExprKind::Unit))
    );
}

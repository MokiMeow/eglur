use eglur_hir::{BinaryOperator, LiteralKind, UnaryOperator};
use eglur_syntax::SyntaxKind;

use super::LoweredExpression;

#[test]
fn binary_operators_preserve_identity_and_distinct_operand_roles() {
    for (symbol, operator) in [
        ("||", BinaryOperator::LogicalOr),
        ("&&", BinaryOperator::LogicalAnd),
        ("==", BinaryOperator::Equal),
        ("!=", BinaryOperator::NotEqual),
        ("<", BinaryOperator::Less),
        ("<=", BinaryOperator::LessEqual),
        (">", BinaryOperator::Greater),
        (">=", BinaryOperator::GreaterEqual),
        ("..", BinaryOperator::Range),
        ("..=", BinaryOperator::RangeInclusive),
        ("+", BinaryOperator::Add),
        ("-", BinaryOperator::Subtract),
        ("*", BinaryOperator::Multiply),
        ("/", BinaryOperator::Divide),
        ("/.", BinaryOperator::FractionalDivide),
        ("%", BinaryOperator::Remainder),
    ] {
        let spelling = format!("left_value {symbol} right_value");
        let source = LoweredExpression::new(&spelling);
        let (left, right) = source.binary(source.root, &spelling, operator);
        source.name(left, "left_value", false);
        source.name(right, "right_value", false);
    }
}

#[test]
fn mixed_precedence_preserves_logical_comparison_and_arithmetic_subtrees() {
    let spelling = "first + second * third < limit && !disabled || fallback";
    let source = LoweredExpression::new(spelling);
    let (and, fallback) = source.binary(source.root, spelling, BinaryOperator::LogicalOr);
    source.name(fallback, "fallback", false);
    let (less, not) = source.binary(
        and,
        "first + second * third < limit && !disabled",
        BinaryOperator::LogicalAnd,
    );
    source.name(
        source.unary(not, "!disabled", UnaryOperator::Not),
        "disabled",
        false,
    );

    let (add, limit) = source.binary(less, "first + second * third < limit", BinaryOperator::Less);
    source.name(limit, "limit", false);
    let (first, multiply) = source.binary(add, "first + second * third", BinaryOperator::Add);
    source.name(first, "first", false);
    let (second, third) = source.binary(multiply, "second * third", BinaryOperator::Multiply);
    source.name(second, "second", false);
    source.name(third, "third", false);
}

#[test]
fn same_precedence_operators_keep_left_association() {
    for (first, first_operator, second, second_operator) in [
        (
            "||",
            BinaryOperator::LogicalOr,
            "||",
            BinaryOperator::LogicalOr,
        ),
        (
            "&&",
            BinaryOperator::LogicalAnd,
            "&&",
            BinaryOperator::LogicalAnd,
        ),
        ("<", BinaryOperator::Less, "!=", BinaryOperator::NotEqual),
        ("-", BinaryOperator::Subtract, "+", BinaryOperator::Add),
        (
            "/.",
            BinaryOperator::FractionalDivide,
            "%",
            BinaryOperator::Remainder,
        ),
    ] {
        let prefix = format!("alpha {first} beta");
        let spelling = format!("{prefix} {second} gamma");
        let source = LoweredExpression::new(&spelling);
        let (nested, gamma) = source.binary(source.root, &spelling, second_operator);
        let (alpha, beta) = source.binary(nested, &prefix, first_operator);
        source.name(alpha, "alpha", false);
        source.name(beta, "beta", false);
        source.name(gamma, "gamma", false);
    }
}

#[test]
fn groups_override_precedence_without_losing_the_group_node() {
    let spelling = "(first + second) * third";
    let source = LoweredExpression::new(spelling);
    let (group, third) = source.binary(source.root, spelling, BinaryOperator::Multiply);
    source.name(third, "third", false);
    let add = source.group(group, "(first + second)");
    let (first, second) = source.binary(add, "first + second", BinaryOperator::Add);
    source.name(first, "first", false);
    source.name(second, "second", false);
}

#[test]
fn range_bounds_preserve_arithmetic_and_comparison_precedence() {
    for (symbol, operator) in [
        ("..", BinaryOperator::Range),
        ("..=", BinaryOperator::RangeInclusive),
    ] {
        let range = format!("lower + offset {symbol} upper - margin");
        let spelling = format!("{range} == expected");
        let source = LoweredExpression::new(&spelling);
        let (range_id, expected) = source.binary(source.root, &spelling, BinaryOperator::Equal);
        source.name(expected, "expected", false);
        let (start, end) = source.binary(range_id, &range, operator);
        let (lower, offset) = source.binary(start, "lower + offset", BinaryOperator::Add);
        let (upper, margin) = source.binary(end, "upper - margin", BinaryOperator::Subtract);
        for (id, name) in [
            (lower, "lower"),
            (offset, "offset"),
            (upper, "upper"),
            (margin, "margin"),
        ] {
            source.name(id, name, false);
        }
    }
}

#[test]
fn unary_operators_preserve_operands_and_noninteger_negation() {
    for (spelling, name, operator) in [
        ("!flag", "flag", UnaryOperator::Not),
        ("-amount", "amount", UnaryOperator::Negate),
        ("*pointer", "pointer", UnaryOperator::Dereference),
    ] {
        let source = LoweredExpression::new(spelling);
        source.name(source.unary(source.root, spelling, operator), name, false);
    }

    let source = LoweredExpression::new("-2.5");
    let value = source.unary(source.root, "-2.5", UnaryOperator::Negate);
    source.literal(value, "2.5", LiteralKind::Float, SyntaxKind::Float);
}

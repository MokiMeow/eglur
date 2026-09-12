use eglur_source::FileSpan;
use eglur_syntax::{SyntaxKind, SyntaxNode};

use super::body::{expression_children, lower_binary_operator};
use super::{Builder, direct_token};
use crate::{
    BinaryOperator, BodyId, Expr, ExprId, ExprKind, InvalidCategory, InvalidOccurrence, Literal,
    LiteralKind, OriginId, UnaryOperator,
};

enum OperatorStep {
    Enter(Option<SyntaxNode>, FileSpan),
    Binary(OriginId, Option<BinaryOperator>),
    Unary(OriginId, Option<UnaryOperator>),
    Group(OriginId),
}

impl Builder {
    pub(super) fn lower_operator_tree(&mut self, body: BodyId, root: SyntaxNode) -> ExprId {
        let span = root.span();
        let mut pending = vec![OperatorStep::Enter(Some(root), span)];
        let mut values = Vec::new();

        // Origins remain preorder; completed expressions keep left-to-right postorder.
        while let Some(step) = pending.pop() {
            let (origin, kind) = match step {
                OperatorStep::Enter(node, fallback) => {
                    if !node.as_ref().is_some_and(|node| {
                        matches!(
                            node.kind(),
                            SyntaxKind::BinaryExpr | SyntaxKind::UnaryExpr | SyntaxKind::GroupExpr
                        )
                    }) {
                        // Non-operator leaves retain their own lowering rules and origins.
                        values.push(self.lower_expr(body, node, fallback));
                        continue;
                    }
                    let node = node.expect("operator entry has a syntax node");
                    let origin = self.written(node.span());
                    if let Some(literal) = self.lower_signed_integer(body, &node, origin) {
                        values.push(literal);
                        continue;
                    }
                    let mut children = expression_children(&node).into_iter();
                    match node.kind() {
                        SyntaxKind::BinaryExpr => {
                            let left = children.next();
                            let right = children.next();
                            pending
                                .push(OperatorStep::Binary(origin, lower_binary_operator(&node)));
                            pending.push(OperatorStep::Enter(right, node.span()));
                            pending.push(OperatorStep::Enter(left, node.span()));
                        }
                        SyntaxKind::UnaryExpr => {
                            let operator = if direct_token(&node, SyntaxKind::Bang).is_some() {
                                Some(UnaryOperator::Not)
                            } else if direct_token(&node, SyntaxKind::Minus).is_some() {
                                Some(UnaryOperator::Negate)
                            } else if direct_token(&node, SyntaxKind::Star).is_some() {
                                Some(UnaryOperator::Dereference)
                            } else {
                                None
                            };
                            pending.push(OperatorStep::Unary(origin, operator));
                            pending.push(OperatorStep::Enter(children.next(), node.span()));
                        }
                        SyntaxKind::GroupExpr => {
                            pending.push(OperatorStep::Group(origin));
                            pending.push(OperatorStep::Enter(children.next(), node.span()));
                        }
                        _ => unreachable!(),
                    }
                    continue;
                }
                OperatorStep::Binary(origin, operator) => {
                    let right = values.pop().expect("binary right operand is lowered");
                    let left = values.pop().expect("binary left operand is lowered");
                    (
                        origin,
                        operator.map(|operator| ExprKind::Binary {
                            operator,
                            left,
                            right,
                        }),
                    )
                }
                OperatorStep::Unary(origin, operator) => {
                    let operand = values.pop().expect("unary operand is lowered");
                    (
                        origin,
                        operator.map(|operator| ExprKind::Unary { operator, operand }),
                    )
                }
                OperatorStep::Group(origin) => {
                    let operand = values.pop().expect("group operand is lowered");
                    (origin, Some(ExprKind::Group(operand)))
                }
            };
            let kind = kind.unwrap_or_else(|| {
                self.invalid
                    .push(InvalidOccurrence::new(InvalidCategory::Expression, origin));
                ExprKind::Invalid
            });
            values.push(self.alloc_expr(body, Expr::new(origin, kind)));
        }
        assert_eq!(values.len(), 1, "operator tree produces one expression");
        values.pop().expect("operator root is lowered")
    }

    fn lower_signed_integer(
        &mut self,
        body: BodyId,
        node: &SyntaxNode,
        written: OriginId,
    ) -> Option<ExprId> {
        if node.kind() != SyntaxKind::UnaryExpr {
            return None;
        }
        let sign = direct_token(node, SyntaxKind::Minus)?;
        let operand = expression_children(node).into_iter().next()?;
        if operand.kind() != SyntaxKind::LiteralExpr {
            return None;
        }
        let token = direct_token(&operand, SyntaxKind::Integer)?;
        // Fold only a directly signed literal, preserving the full signed minimum.
        let range =
            eglur_source::ByteRange::new(sign.span().range().start(), token.span().range().end())
                .expect("signed literal tokens are source ordered");
        let origin = self.written(FileSpan::new(node.span().file(), range));
        let literal = Literal::new(
            LiteralKind::Integer,
            format!("-{}", token.text()).into_boxed_str(),
            origin,
        );
        Some(self.alloc_expr(body, Expr::new(written, ExprKind::Literal(literal))))
    }
}

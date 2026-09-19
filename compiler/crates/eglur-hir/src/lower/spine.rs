use eglur_source::FileSpan;
use eglur_syntax::{SyntaxKind, SyntaxNode};

use super::body::{expression_children, lower_binary_operator};
use super::calls::{CallStep, PostfixApplication};
use super::traversal::{Step, Traversal};
use super::{Builder, direct_token, first_name_token};
use crate::{
    BinaryOperator, BodyId, CallArgument, CallExpr, CallSyntax, Expr, ExprId, ExprKind,
    InvalidCategory, InvalidOccurrence, Literal, LiteralKind, Mutability, OriginId, UnaryOperator,
};

pub(super) enum SpineStep {
    Binary(OriginId, Option<BinaryOperator>),
    Unary(OriginId, Option<UnaryOperator>),
    Group(OriginId),
    Reference(OriginId, Mutability),
    Member(OriginId, SyntaxNode),
    Projection(OriginId, SyntaxNode),
    Try(OriginId),
    DTypeCall(OriginId, SyntaxNode),
    ReduceBy(OriginId, SyntaxNode),
    PostfixCallable(PostfixApplication, FileSpan),
    PostfixInput(PendingPostfix, FileSpan),
}

pub(super) struct PendingPostfix {
    span: FileSpan,
    syntax: CallSyntax,
    callee: ExprId,
    inputs: std::vec::IntoIter<SyntaxNode>,
    arguments: Vec<CallArgument>,
}

impl Builder {
    pub(super) fn enter_expr(
        &mut self,
        body: BodyId,
        node: Option<SyntaxNode>,
        fallback: FileSpan,
        traversal: &mut Traversal,
    ) {
        let Some(node) = node else {
            traversal.values.push(self.invalid_expr(body, fallback));
            return;
        };
        if node.kind() == SyntaxKind::CallExpr && !self.is_catalog_call(&node) {
            if let Some(application) = PostfixApplication::from_syntax(&node) {
                let callable = application.callable.clone();
                traversal.push(
                    body,
                    Step::Spine(SpineStep::PostfixCallable(application, node.span())),
                );
                traversal.expression(body, Some(callable), node.span());
            } else {
                let callee = expression_children(&node).into_iter().next();
                let span = node.span();
                traversal.push(body, Step::Call(CallStep::Callee(node)));
                traversal.expression(body, callee, span);
            }
            return;
        }
        if !matches!(
            node.kind(),
            SyntaxKind::BinaryExpr
                | SyntaxKind::UnaryExpr
                | SyntaxKind::GroupExpr
                | SyntaxKind::RefExpr
                | SyntaxKind::MemberExpr
                | SyntaxKind::TupleProjectionExpr
                | SyntaxKind::TryExpr
                | SyntaxKind::DTypeBracketCallExpr
                | SyntaxKind::ReduceByCandidateExpr
        ) {
            self.enter_body_expr(body, node, traversal);
            return;
        }

        // Written spine origins precede children; calls keep their own origin timing.
        let origin = self.written(node.span());
        if let Some(literal) = self.lower_signed_integer(body, &node, origin) {
            traversal.values.push(literal);
            return;
        }
        let span = node.span();
        let mut children = expression_children(&node).into_iter();
        let first = children.next();
        let finish = match node.kind() {
            SyntaxKind::BinaryExpr => {
                traversal.push(
                    body,
                    Step::Spine(SpineStep::Binary(origin, lower_binary_operator(&node))),
                );
                traversal.expression(body, children.next(), span);
                traversal.expression(body, first, span);
                return;
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
                SpineStep::Unary(origin, operator)
            }
            SyntaxKind::GroupExpr => SpineStep::Group(origin),
            SyntaxKind::RefExpr => SpineStep::Reference(
                origin,
                if direct_token(&node, SyntaxKind::MutKw).is_some() {
                    Mutability::Mutable
                } else {
                    Mutability::Immutable
                },
            ),
            SyntaxKind::MemberExpr => SpineStep::Member(origin, node),
            SyntaxKind::TupleProjectionExpr => SpineStep::Projection(origin, node),
            SyntaxKind::TryExpr => SpineStep::Try(origin),
            SyntaxKind::DTypeBracketCallExpr => SpineStep::DTypeCall(origin, node),
            SyntaxKind::ReduceByCandidateExpr => SpineStep::ReduceBy(origin, node),
            _ => unreachable!(),
        };
        traversal.push(body, Step::Spine(finish));
        traversal.expression(body, first, span);
    }

    pub(super) fn resume_spine(
        &mut self,
        body: BodyId,
        step: SpineStep,
        traversal: &mut Traversal,
    ) {
        let (origin, kind) = match step {
            SpineStep::Binary(origin, operator) => {
                let right = traversal
                    .values
                    .pop()
                    .expect("binary right operand is lowered");
                let left = traversal
                    .values
                    .pop()
                    .expect("binary left operand is lowered");
                (
                    origin,
                    operator.map(|operator| ExprKind::Binary {
                        operator,
                        left,
                        right,
                    }),
                )
            }
            SpineStep::Unary(origin, operator) => {
                let operand = traversal.values.pop().expect("unary operand is lowered");
                (
                    origin,
                    operator.map(|operator| ExprKind::Unary { operator, operand }),
                )
            }
            SpineStep::Group(origin) => {
                let operand = traversal.values.pop().expect("group operand is lowered");
                (origin, Some(ExprKind::Group(operand)))
            }
            SpineStep::Reference(origin, mutability) => {
                let value = traversal
                    .values
                    .pop()
                    .expect("reference operand is lowered");
                (origin, Some(ExprKind::Reference { mutability, value }))
            }
            SpineStep::Member(origin, node) => {
                let receiver = traversal.values.pop().expect("member receiver is lowered");
                let member = self.lower_name_token(first_name_token(&node), node.span());
                (origin, Some(ExprKind::Member { receiver, member }))
            }
            SpineStep::Projection(origin, node) => {
                let receiver = traversal
                    .values
                    .pop()
                    .expect("projection receiver is lowered");
                let kind = direct_token(&node, SyntaxKind::TupleIndex)
                    .filter(|token| !token.is_missing())
                    .map(|index| ExprKind::TupleProjection {
                        receiver,
                        index: index.text().into(),
                    });
                (origin, kind)
            }
            SpineStep::Try(origin) => {
                let value = traversal.values.pop().expect("try operand is lowered");
                (origin, Some(ExprKind::Try(value)))
            }
            SpineStep::DTypeCall(origin, node) => {
                let callee = traversal
                    .values
                    .pop()
                    .expect("dtype call target is lowered");
                self.schedule_dtype_call(body, origin, node, callee, traversal);
                return;
            }
            SpineStep::ReduceBy(origin, node) => {
                let receiver = traversal
                    .values
                    .pop()
                    .expect("reduce_by receiver is lowered");
                self.schedule_reduce_by(body, origin, node, receiver, traversal);
                return;
            }
            SpineStep::PostfixCallable(application, span) => {
                let callee = traversal.values.pop().expect("postfix callable is lowered");
                let call = PendingPostfix {
                    span,
                    syntax: application.syntax,
                    callee,
                    inputs: application.inputs.into_iter(),
                    arguments: Vec::new(),
                };
                self.continue_postfix(body, call, traversal);
                return;
            }
            SpineStep::PostfixInput(mut call, span) => {
                let value = traversal.values.pop().expect("postfix input is lowered");
                call.arguments.push(CallArgument {
                    name: None,
                    value,
                    origin: self.written(span),
                });
                self.continue_postfix(body, call, traversal);
                return;
            }
        };
        let kind = kind.unwrap_or_else(|| {
            self.invalid
                .push(InvalidOccurrence::new(InvalidCategory::Expression, origin));
            ExprKind::Invalid
        });
        traversal
            .values
            .push(self.alloc_expr(body, Expr::new(origin, kind)));
    }

    fn continue_postfix(
        &mut self,
        body: BodyId,
        mut call: PendingPostfix,
        traversal: &mut Traversal,
    ) {
        if let Some(input) = call.inputs.next() {
            let span = input.span();
            let fallback = call.span;
            traversal.push(body, Step::Spine(SpineStep::PostfixInput(call, span)));
            traversal.expression(body, Some(input), fallback);
            return;
        }
        let origin = self.derived(
            call.span,
            call.syntax
                .call_desugaring()
                .expect("postfix application is a desugaring"),
        );
        let partial_application = self.expr_is_hole(call.callee)
            || call
                .arguments
                .iter()
                .any(|argument| self.expr_is_hole(argument.value));
        let kind = ExprKind::Call(CallExpr {
            callee: call.callee,
            arguments: call.arguments.into_boxed_slice(),
            type_arguments: Box::new([]),
            syntax: call.syntax,
            partial_application,
        });
        traversal
            .values
            .push(self.alloc_expr(body, Expr::new(origin, kind)));
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

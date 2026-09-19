use eglur_syntax::{SyntaxKind, SyntaxNode};

use super::body::{BodyStep, expression_children};
use super::traversal::{Step, Traversal, take_tail};
use super::{
    Builder, direct_child, direct_children, direct_nodes, direct_token, direct_tokens,
    is_expression_kind,
};
use crate::{
    BodyId, Expr, ExprKind, IfExpr, LambdaExpr, LambdaSyntax, MatchArm, MatchExpr, MatchGuard,
    OriginId, Parameter, PatternId,
};

pub(super) enum ControlStep {
    If(OriginId, usize),
    Match(OriginId, usize),
    Arm(SyntaxNode),
    ArmValues(OriginId, PatternId, Option<OriginId>),
    Lambda(OriginId, PendingLambda),
}

pub(super) struct PendingLambda {
    body: BodyId,
    syntax: LambdaSyntax,
    parameters: Box<[Parameter]>,
    statement_count: usize,
    has_tail: bool,
}

impl Builder {
    pub(super) fn schedule_control(
        &mut self,
        body: BodyId,
        origin: OriginId,
        node: &SyntaxNode,
        traversal: &mut Traversal,
    ) {
        match node.kind() {
            SyntaxKind::IfExpr => {
                let expressions = expression_children(node);
                let count = expressions.len().clamp(2, 4);
                traversal.push(body, Step::Control(ControlStep::If(origin, count)));
                for index in (0..count).rev() {
                    traversal.expression(body, expressions.get(index).cloned(), node.span());
                }
            }
            SyntaxKind::MatchExpr => {
                let arms = direct_children(node, SyntaxKind::MatchArm);
                traversal.push(body, Step::Control(ControlStep::Match(origin, arms.len())));
                for arm in arms.into_iter().rev() {
                    traversal.push(body, Step::Control(ControlStep::Arm(arm)));
                }
                traversal.expression(
                    body,
                    expression_children(node).into_iter().next(),
                    node.span(),
                );
            }
            SyntaxKind::LambdaExpr => self.schedule_lambda(body, origin, node, traversal),
            _ => unreachable!("control syntax was classified at expression entry"),
        }
    }

    fn schedule_lambda(
        &mut self,
        outer_body: BodyId,
        origin: OriginId,
        node: &SyntaxNode,
        traversal: &mut Traversal,
    ) {
        let body = self.new_body();
        let explicit = direct_token(node, SyntaxKind::LogicalOr).is_some()
            || direct_token(node, SyntaxKind::Pipe).is_some();
        let parameters = direct_child(node, SyntaxKind::LambdaParameterList)
            .map(|parameters| {
                direct_children(&parameters, SyntaxKind::Parameter)
                    .into_iter()
                    .map(|parameter| {
                        let origin = self.written(parameter.span());
                        let token = direct_tokens(&parameter).into_iter().find(|token| {
                            matches!(
                                token.kind(),
                                SyntaxKind::Identifier | SyntaxKind::Underscore
                            )
                        });
                        let pattern =
                            self.lower_binding_pattern_from_token(body, token, parameter.span());
                        let ty = direct_child(&parameter, SyntaxKind::TypeExpr)
                            .map(|ty| self.lower_type(Some(ty), parameter.span()));
                        Parameter {
                            pattern,
                            ty,
                            origin,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let lambda_body = direct_child(node, SyntaxKind::LambdaBody);
        let statements = lambda_body
            .as_ref()
            .map(|lambda_body| {
                direct_nodes(lambda_body)
                    .into_iter()
                    .filter(|child| !is_expression_kind(child.kind()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let tail = lambda_body
            .as_ref()
            .and_then(|lambda_body| expression_children(lambda_body).into_iter().last());
        traversal.push(
            outer_body,
            Step::Control(ControlStep::Lambda(
                origin,
                PendingLambda {
                    body,
                    syntax: if explicit {
                        LambdaSyntax::Explicit
                    } else {
                        LambdaSyntax::Implicit
                    },
                    parameters: parameters.into_boxed_slice(),
                    statement_count: statements.len(),
                    has_tail: tail.is_some(),
                },
            )),
        );
        if let Some(tail) = tail {
            traversal.expression(body, Some(tail), node.span());
        }
        for statement in statements.into_iter().rev() {
            traversal.push(body, Step::Body(BodyStep::Statement(statement)));
        }
    }

    pub(super) fn resume_control(
        &mut self,
        body: BodyId,
        step: ControlStep,
        traversal: &mut Traversal,
    ) {
        let (origin, kind) = match step {
            ControlStep::If(origin, count) => {
                let values = take_tail(&mut traversal.values, count);
                (
                    origin,
                    ExprKind::If(IfExpr {
                        condition: values[0],
                        then_branch: values[1],
                        else_branch: values.get(2).copied(),
                        unknown_branch: values.get(3).copied(),
                    }),
                )
            }
            ControlStep::Match(origin, count) => {
                let arms = take_tail(&mut traversal.arms, count);
                (
                    origin,
                    ExprKind::Match(MatchExpr {
                        scrutinee: traversal.value(),
                        arms,
                    }),
                )
            }
            ControlStep::Arm(node) => {
                let origin = self.written(node.span());
                let pattern =
                    self.lower_pattern(body, direct_child(&node, SyntaxKind::Pattern), node.span());
                let guard = direct_child(&node, SyntaxKind::MatchGuard);
                let guard_origin = guard.as_ref().map(|guard| self.written(guard.span()));
                traversal.push(
                    body,
                    Step::Control(ControlStep::ArmValues(origin, pattern, guard_origin)),
                );
                traversal.expression(
                    body,
                    expression_children(&node).into_iter().last(),
                    node.span(),
                );
                if let Some(guard) = guard {
                    traversal.expression(
                        body,
                        expression_children(&guard).into_iter().next(),
                        guard.span(),
                    );
                }
                return;
            }
            ControlStep::ArmValues(origin, pattern, guard_origin) => {
                let value = traversal.value();
                let guard = guard_origin.map(|origin| MatchGuard {
                    condition: traversal.value(),
                    origin,
                });
                traversal.arms.push(MatchArm {
                    pattern,
                    guard,
                    value,
                    origin,
                });
                return;
            }
            ControlStep::Lambda(origin, lambda) => {
                let tail = lambda.has_tail.then(|| traversal.value());
                let statements = take_tail(&mut traversal.statements, lambda.statement_count);
                (
                    origin,
                    ExprKind::Lambda(LambdaExpr {
                        body: lambda.body,
                        syntax: lambda.syntax,
                        parameters: lambda.parameters,
                        statements,
                        tail,
                    }),
                )
            }
        };
        traversal
            .values
            .push(self.alloc_expr(body, Expr::new(origin, kind)));
    }
}

use eglur_source::{ByteRange, FileSpan};
use eglur_syntax::{SyntaxKind, SyntaxNode};

use super::body::expression_children;
use super::{Builder, direct_child, direct_children, direct_nodes, direct_token, direct_tokens};
use crate::{
    AxisExpr, AxisExprKind, CallArgument, CallExpr, CallSyntax, DesugaringKind, Expr, ExprKind,
    InvalidCategory, Name, Path, ReduceByCandidateExpr, ReduceByOrdinaryCandidate, ReduceByPhase,
    ReduceByPhasesCandidate, ReduceBySingleCandidate, ReduceBySingleValue, TensorSchemaExpr,
    TensorSchemaFamily, TransformAxisList,
};

use super::traversal::{Step, Traversal, take_tail};

pub(super) enum ReduceStep {
    Ordinary(crate::OriginId, SyntaxNode, crate::ExprId, usize),
    Phases(crate::OriginId, SyntaxNode, crate::ExprId, usize),
    Phase(SyntaxNode),
    Reducer(crate::OriginId, Box<[Name]>),
}

enum AxisBinaryOperator {
    Add,
    Multiply,
}

enum AxisStep {
    Enter(Option<SyntaxNode>, FileSpan),
    Binary(crate::OriginId, Option<AxisBinaryOperator>),
    Group(crate::OriginId),
}

impl Builder {
    pub(super) fn lower_tensor_schema(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        origin: crate::OriginId,
    ) -> ExprKind {
        let family = direct_tokens(node)
            .into_iter()
            .find(|token| token.kind() == SyntaxKind::Identifier)
            .and_then(|token| match token.text() {
                "view" => Some(TensorSchemaFamily::View),
                "rearrange" => Some(TensorSchemaFamily::Rearrange),
                "inflate" => Some(TensorSchemaFamily::Inflate),
                "reduce" => Some(TensorSchemaFamily::Reduce),
                "contract" => Some(TensorSchemaFamily::Contract),
                "einop" => Some(TensorSchemaFamily::Einop),
                _ => None,
            });
        let Some(family) = family else {
            self.invalid.push(crate::InvalidOccurrence::new(
                InvalidCategory::Expression,
                origin,
            ));
            return ExprKind::Invalid;
        };
        let mut sides = direct_children(node, SyntaxKind::TransformOperandList).into_iter();
        let operands = sides
            .next()
            .map(|side| self.lower_transform_lists(body, &side))
            .unwrap_or_default();
        let outputs = sides
            .next()
            .map(|side| self.lower_transform_lists(body, &side))
            .unwrap_or_default();
        ExprKind::TensorSchema(TensorSchemaExpr {
            family,
            operands: operands.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
        })
    }

    pub(super) fn schedule_reduce_by(
        &mut self,
        body: crate::BodyId,
        origin: crate::OriginId,
        node: SyntaxNode,
        receiver: crate::ExprId,
        traversal: &mut Traversal,
    ) {
        let arguments = direct_child(&node, SyntaxKind::ReduceByArgumentList);
        let phases = arguments
            .as_ref()
            .map(|arguments| direct_children(arguments, SyntaxKind::ReduceByPhase))
            .unwrap_or_default();
        if phases.is_empty() {
            let count = arguments
                .as_ref()
                .map_or(0, |arguments| direct_nodes(arguments).len());
            traversal.push(
                body,
                Step::Reduce(ReduceStep::Ordinary(origin, node, receiver, count)),
            );
            self.schedule_arguments(body, arguments, traversal);
        } else {
            traversal.push(
                body,
                Step::Reduce(ReduceStep::Phases(origin, node, receiver, phases.len())),
            );
            for phase in phases.into_iter().rev() {
                traversal.push(body, Step::Reduce(ReduceStep::Phase(phase)));
            }
        }
    }

    pub(super) fn resume_reduce(
        &mut self,
        body: crate::BodyId,
        step: ReduceStep,
        traversal: &mut Traversal,
    ) {
        let (origin, kind) = match step {
            ReduceStep::Phase(phase) => {
                let origin = self.written(phase.span());
                let axes = direct_tokens(&phase)
                    .into_iter()
                    .filter(|token| token.kind() == SyntaxKind::Identifier)
                    .map(|token| self.lower_name_token(Some(token), phase.span()))
                    .collect::<Vec<_>>();
                traversal.push(
                    body,
                    Step::Reduce(ReduceStep::Reducer(origin, axes.into_boxed_slice())),
                );
                traversal.expression(
                    body,
                    expression_children(&phase).into_iter().next(),
                    phase.span(),
                );
                return;
            }
            ReduceStep::Reducer(origin, axes) => {
                let reducer = traversal.value();
                traversal.phases.push(ReduceByPhase {
                    axes,
                    reducer,
                    origin,
                });
                return;
            }
            ReduceStep::Ordinary(origin, node, receiver, count) => {
                let arguments = take_tail(&mut traversal.arguments, count).into_vec();
                (
                    origin,
                    self.finish_reduce_by(body, &node, receiver, arguments),
                )
            }
            ReduceStep::Phases(origin, node, receiver, count) => {
                let phases = take_tail(&mut traversal.phases, count).into_vec();
                (
                    origin,
                    self.finish_phase_candidates(body, &node, receiver, phases),
                )
            }
        };
        traversal
            .values
            .push(self.alloc_expr(body, Expr::new(origin, kind)));
    }

    fn finish_reduce_by(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        receiver: crate::ExprId,
        ordinary_arguments: Vec<CallArgument>,
    ) -> ExprKind {
        let single_reducer = if ordinary_arguments.len() == 1
            && ordinary_arguments[0].name.is_none()
        {
            Some(ReduceBySingleCandidate {
                value: ReduceBySingleValue::Expression(ordinary_arguments[0].value),
                origin: self.derived(node.span(), DesugaringKind::ReduceBySingleReducerCandidate),
            })
        } else {
            None
        };
        ExprKind::ReduceByCandidate(ReduceByCandidateExpr {
            receiver,
            arguments_origin: self.reduce_by_arguments_origin(node),
            ordinary: Some(self.lower_ordinary_reduce_by_candidate(
                body,
                node,
                receiver,
                ordinary_arguments,
                Vec::new(),
            )),
            single_reducer,
            ordered_phases: None,
        })
    }

    fn finish_phase_candidates(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        receiver: crate::ExprId,
        phases: Vec<ReduceByPhase>,
    ) -> ExprKind {
        let ordered_origin =
            self.derived(node.span(), DesugaringKind::ReduceByOrderedPhasesCandidate);

        let ordinary = phases.iter().all(|phase| phase.axes.len() == 1).then(|| {
            let mut candidate_names = Vec::with_capacity(phases.len());
            let arguments = phases
                .iter()
                .map(|phase| {
                    let axis = phase.axes[0].clone();
                    let axis_origin = axis.origin();
                    let axis_value = self.alloc_expr(
                        body,
                        Expr::new(
                            axis_origin,
                            ExprKind::Name(Path::new(Box::new([axis]), axis_origin)),
                        ),
                    );
                    candidate_names.push(axis_value);
                    let pair = self.alloc_expr(
                        body,
                        Expr::new(
                            phase.origin,
                            ExprKind::Tuple(Box::new([axis_value, phase.reducer])),
                        ),
                    );
                    CallArgument {
                        name: None,
                        value: pair,
                        origin: phase.origin,
                    }
                })
                .collect();
            self.lower_ordinary_reduce_by_candidate(
                body,
                node,
                receiver,
                arguments,
                candidate_names,
            )
        });
        let single_reducer = if phases.len() == 1 && phases[0].axes.len() == 1 {
            Some(ReduceBySingleCandidate {
                value: ReduceBySingleValue::PhasePair {
                    axis: phases[0].axes[0].clone(),
                    reducer: phases[0].reducer,
                    origin: phases[0].origin,
                },
                origin: self.derived(node.span(), DesugaringKind::ReduceBySingleReducerCandidate),
            })
        } else {
            None
        };
        ExprKind::ReduceByCandidate(ReduceByCandidateExpr {
            receiver,
            arguments_origin: self.reduce_by_arguments_origin(node),
            ordinary,
            single_reducer,
            ordered_phases: Some(ReduceByPhasesCandidate {
                phases: phases.into_boxed_slice(),
                origin: ordered_origin,
            }),
        })
    }

    fn lower_ordinary_reduce_by_candidate(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        receiver: crate::ExprId,
        arguments: Vec<CallArgument>,
        candidate_names: Vec<crate::ExprId>,
    ) -> ReduceByOrdinaryCandidate {
        let origin = self.derived(node.span(), DesugaringKind::ReduceByOrdinaryCandidate);
        let member = direct_tokens(node)
            .into_iter()
            .find(|token| token.kind() == SyntaxKind::Identifier && token.text() == "reduce_by");
        let member = self.lower_name_token(member, node.span());
        let callee = self.alloc_expr(
            body,
            Expr::new(origin, ExprKind::Member { receiver, member }),
        );
        let partial_application = arguments
            .iter()
            .any(|argument| self.expr_is_hole(argument.value));
        ReduceByOrdinaryCandidate {
            call: CallExpr {
                callee,
                arguments: arguments.into_boxed_slice(),
                type_arguments: Box::new([]),
                syntax: CallSyntax::Direct,
                partial_application,
            },
            candidate_names: candidate_names.into_boxed_slice(),
            origin,
        }
    }

    fn reduce_by_arguments_origin(&mut self, node: &SyntaxNode) -> crate::OriginId {
        let tokens = direct_tokens(node);
        let span = tokens
            .iter()
            .find(|token| token.kind() == SyntaxKind::LeftParen)
            .zip(
                tokens
                    .iter()
                    .rev()
                    .find(|token| token.kind() == SyntaxKind::RightParen),
            )
            .and_then(|(left, right)| {
                ByteRange::new(left.span().range().start(), right.span().range().end())
                    .map(|range| FileSpan::new(left.span().file(), range))
            })
            .unwrap_or_else(|| node.span());
        self.written(span)
    }

    fn lower_transform_lists(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
    ) -> Vec<TransformAxisList> {
        direct_children(node, SyntaxKind::TransformAxisList)
            .into_iter()
            .map(|list| TransformAxisList {
                origin: self.written(list.span()),
                axes: direct_nodes(&list)
                    .into_iter()
                    .map(|axis| self.lower_axis(body, Some(axis), list.span()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })
            .collect()
    }

    fn lower_axis(
        &mut self,
        body: crate::BodyId,
        node: Option<SyntaxNode>,
        fallback: FileSpan,
    ) -> crate::AxisExprId {
        let mut pending = vec![AxisStep::Enter(node, fallback)];
        let mut values = Vec::new();

        while let Some(step) = pending.pop() {
            let (origin, kind) = match step {
                AxisStep::Enter(node, fallback) => {
                    let Some(node) = node else {
                        values.push(self.invalid_axis(body, fallback));
                        continue;
                    };
                    // Preserve origin preorder and left-to-right child allocation order.
                    let origin = self.written(node.span());
                    let kind = match node.kind() {
                        SyntaxKind::AxisBinaryExpr => {
                            let mut children = direct_nodes(&node).into_iter();
                            let left = children.next();
                            let right = children.next();
                            let operator = if direct_token(&node, SyntaxKind::Plus).is_some() {
                                Some(AxisBinaryOperator::Add)
                            } else if direct_token(&node, SyntaxKind::Star).is_some() {
                                Some(AxisBinaryOperator::Multiply)
                            } else {
                                None
                            };
                            pending.push(AxisStep::Binary(origin, operator));
                            pending.push(AxisStep::Enter(right, node.span()));
                            pending.push(AxisStep::Enter(left, node.span()));
                            continue;
                        }
                        SyntaxKind::AxisAtom => {
                            if let Some(nested) = direct_nodes(&node).into_iter().find(|child| {
                                matches!(
                                    child.kind(),
                                    SyntaxKind::AxisBinaryExpr | SyntaxKind::AxisAtom
                                )
                            }) {
                                pending.push(AxisStep::Group(origin));
                                pending.push(AxisStep::Enter(Some(nested), node.span()));
                                continue;
                            }
                            direct_tokens(&node).into_iter().next().and_then(|token| {
                                match token.kind() {
                                    SyntaxKind::Identifier => Some(AxisExprKind::Name(
                                        self.lower_name_token(Some(token), node.span()),
                                    )),
                                    SyntaxKind::Integer => Some(AxisExprKind::Integer(
                                        self.lower_literal_token(Some(token), node.span()),
                                    )),
                                    SyntaxKind::AxisPackAnonymous => Some(AxisExprKind::Pack(None)),
                                    SyntaxKind::AxisPackNamed => {
                                        let text = token
                                            .text()
                                            .strip_prefix("...")
                                            .unwrap_or(token.text());
                                        let name = Name::valid(
                                            text.into(),
                                            false,
                                            self.written(token.span()),
                                        );
                                        Some(AxisExprKind::Pack(Some(name)))
                                    }
                                    _ => None,
                                }
                            })
                        }
                        _ => None,
                    };
                    (origin, kind)
                }
                AxisStep::Binary(origin, operator) => {
                    let right = values.pop().expect("right axis is lowered");
                    let left = values.pop().expect("left axis is lowered");
                    let kind = operator.map(|operator| match operator {
                        AxisBinaryOperator::Add => AxisExprKind::Add { left, right },
                        AxisBinaryOperator::Multiply => AxisExprKind::Multiply { left, right },
                    });
                    (origin, kind)
                }
                AxisStep::Group(origin) => {
                    let inner = values.pop().expect("grouped axis is lowered");
                    (origin, Some(AxisExprKind::Group(inner)))
                }
            };
            let kind = kind.unwrap_or_else(|| {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::TensorAxis,
                    origin,
                ));
                AxisExprKind::Invalid
            });
            values.push(self.alloc_axis(body, AxisExpr::new(origin, kind)));
        }
        assert_eq!(values.len(), 1, "axis tree produces one result");
        values.pop().expect("axis root is lowered")
    }

    fn invalid_axis(
        &mut self,
        body: crate::BodyId,
        span: eglur_source::FileSpan,
    ) -> crate::AxisExprId {
        let origin = self.record_invalid(InvalidCategory::TensorAxis, span);
        self.alloc_axis(body, AxisExpr::new(origin, AxisExprKind::Invalid))
    }
}

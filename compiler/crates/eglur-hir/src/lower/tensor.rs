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

    pub(super) fn finish_reduce_by(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        receiver: crate::ExprId,
    ) -> ExprKind {
        let arguments = direct_child(node, SyntaxKind::ReduceByArgumentList);
        let phases = arguments
            .as_ref()
            .map(|arguments| direct_children(arguments, SyntaxKind::ReduceByPhase))
            .unwrap_or_default();

        if !phases.is_empty() {
            return self.lower_phase_candidates(body, node, receiver, phases);
        }

        let ordinary_arguments = arguments
            .as_ref()
            .map(|arguments| self.lower_call_arguments(body, arguments))
            .unwrap_or_default();
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

    fn lower_phase_candidates(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        receiver: crate::ExprId,
        phase_nodes: Vec<SyntaxNode>,
    ) -> ExprKind {
        let phases = phase_nodes
            .into_iter()
            .map(|phase| {
                let origin = self.written(phase.span());
                let axes = direct_tokens(&phase)
                    .into_iter()
                    .filter(|token| token.kind() == SyntaxKind::Identifier)
                    .map(|token| self.lower_name_token(Some(token), phase.span()))
                    .collect::<Vec<_>>();
                let reducer = self.lower_expr(
                    body,
                    expression_children(&phase).into_iter().next(),
                    phase.span(),
                );
                ReduceByPhase {
                    axes: axes.into_boxed_slice(),
                    reducer,
                    origin,
                }
            })
            .collect::<Vec<_>>();
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
        fallback: eglur_source::FileSpan,
    ) -> crate::AxisExprId {
        let Some(node) = node else {
            return self.invalid_axis(body, fallback);
        };
        let origin = self.written(node.span());
        let kind = match node.kind() {
            SyntaxKind::AxisBinaryExpr => {
                let mut children = direct_nodes(&node).into_iter();
                let left = self.lower_axis(body, children.next(), node.span());
                let right = self.lower_axis(body, children.next(), node.span());
                if direct_token(&node, SyntaxKind::Plus).is_some() {
                    AxisExprKind::Add { left, right }
                } else if direct_token(&node, SyntaxKind::Star).is_some() {
                    AxisExprKind::Multiply { left, right }
                } else {
                    self.invalid.push(crate::InvalidOccurrence::new(
                        InvalidCategory::TensorAxis,
                        origin,
                    ));
                    AxisExprKind::Invalid
                }
            }
            SyntaxKind::AxisAtom => {
                if let Some(nested) = direct_nodes(&node).into_iter().find(|child| {
                    matches!(
                        child.kind(),
                        SyntaxKind::AxisBinaryExpr | SyntaxKind::AxisAtom
                    )
                }) {
                    AxisExprKind::Group(self.lower_axis(body, Some(nested), node.span()))
                } else if let Some(token) = direct_tokens(&node).into_iter().next() {
                    match token.kind() {
                        SyntaxKind::Identifier => {
                            AxisExprKind::Name(self.lower_name_token(Some(token), node.span()))
                        }
                        SyntaxKind::Integer => AxisExprKind::Integer(
                            self.lower_literal_token(Some(token), node.span()),
                        ),
                        SyntaxKind::AxisPackAnonymous => AxisExprKind::Pack(None),
                        SyntaxKind::AxisPackNamed => {
                            let text = token.text().strip_prefix("...").unwrap_or(token.text());
                            let name = Name::valid(text.into(), false, self.written(token.span()));
                            AxisExprKind::Pack(Some(name))
                        }
                        _ => {
                            self.invalid.push(crate::InvalidOccurrence::new(
                                InvalidCategory::TensorAxis,
                                origin,
                            ));
                            AxisExprKind::Invalid
                        }
                    }
                } else {
                    self.invalid.push(crate::InvalidOccurrence::new(
                        InvalidCategory::TensorAxis,
                        origin,
                    ));
                    AxisExprKind::Invalid
                }
            }
            _ => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::TensorAxis,
                    origin,
                ));
                AxisExprKind::Invalid
            }
        };
        self.alloc_axis(body, AxisExpr::new(origin, kind))
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

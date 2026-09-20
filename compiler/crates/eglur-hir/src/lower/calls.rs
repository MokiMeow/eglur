use eglur_syntax::{SyntaxKind, SyntaxNode};

use super::body::expression_children;
use super::{
    Builder, direct_child, direct_nodes, direct_token, first_name_token, is_expression_kind,
};
use crate::{
    CallArgument, CallExpr, CallSyntax, DTypeCallExpr, DesugaringKind, Expr, ExprId, ExprKind,
};

pub(super) struct PostfixApplication {
    pub syntax: CallSyntax,
    pub callable: SyntaxNode,
    pub inputs: Vec<SyntaxNode>,
}

impl PostfixApplication {
    pub(super) fn from_syntax(node: &SyntaxNode) -> Option<Self> {
        let callee = expression_children(node).into_iter().next()?;
        if callee.kind() != SyntaxKind::MemberExpr
            || direct_child(node, SyntaxKind::TypeArgumentList).is_some()
        {
            return None;
        }
        let member = first_name_token(&callee)?;
        let syntax = match member.text() {
            "then" => CallSyntax::PostfixThen,
            "apply" => CallSyntax::PostfixApply,
            _ => return None,
        };
        let arguments = direct_nodes(&direct_child(node, SyntaxKind::ArgumentList)?);
        if arguments.len() != 1 || !is_expression_kind(arguments[0].kind()) {
            return None;
        }
        let receiver = expression_children(&callee).into_iter().next()?;
        let inputs = match syntax {
            CallSyntax::PostfixThen => vec![receiver],
            CallSyntax::PostfixApply if receiver.kind() == SyntaxKind::TupleExpr => {
                expression_children(&receiver)
            }
            _ => return None,
        };
        Some(Self {
            syntax,
            callable: arguments.into_iter().next()?,
            inputs,
        })
    }
}

use super::traversal::{Step, Traversal, take_tail};
use crate::{BodyId, CatalogCallExpr, Name, OriginId, TypeRefId};
use eglur_source::FileSpan;

pub(super) enum CallStep {
    Callee(SyntaxNode),
    Finish(SyntaxNode, ExprId, Box<[TypeRefId]>, usize, CallSyntax),
    Argument(SyntaxNode, FileSpan),
    ArgumentValue(OriginId, Option<Name>),
    TrailingArgument(FileSpan),
    DType(OriginId, ExprId, TypeRefId, usize),
    Catalog(OriginId, crate::Path, Box<[TypeRefId]>, Name, usize),
}

impl Builder {
    pub(super) fn call_has_holes(&self, callee: ExprId, arguments: &[CallArgument]) -> bool {
        std::iter::once(callee)
            .chain(arguments.iter().map(|argument| argument.value))
            .any(|id| {
                matches!(
                    self.bodies[id.body().index()].expressions[id.index()].kind(),
                    ExprKind::Hole
                )
            })
    }

    pub(super) fn schedule_arguments(
        &mut self,
        body: BodyId,
        list: Option<SyntaxNode>,
        traversal: &mut Traversal,
    ) {
        let Some(list) = list else { return };
        let nodes = direct_nodes(&list);
        for node in nodes.into_iter().rev() {
            traversal.push(body, Step::Call(CallStep::Argument(node, list.span())));
        }
    }

    pub(super) fn schedule_dtype_call(
        &mut self,
        body: BodyId,
        origin: OriginId,
        node: SyntaxNode,
        callee: ExprId,
        traversal: &mut Traversal,
    ) {
        let dtype = direct_child(&node, SyntaxKind::TypeArgumentList)
            .and_then(|types| self.lower_type_list_container(&types).into_iter().next())
            .unwrap_or_else(|| self.lower_type(None, node.span()));
        let arguments = direct_child(&node, SyntaxKind::ArgumentList);
        let count = arguments
            .as_ref()
            .map_or(0, |list| direct_nodes(list).len());
        traversal.push(
            body,
            Step::Call(CallStep::DType(origin, callee, dtype, count)),
        );
        self.schedule_arguments(body, arguments, traversal);
    }

    pub(super) fn schedule_catalog_call(
        &mut self,
        body: BodyId,
        origin: OriginId,
        node: &SyntaxNode,
        traversal: &mut Traversal,
    ) {
        let owner_node = expression_children(node).into_iter().next();
        let owner = self.lower_expr_path(owner_node.as_ref(), node.span());
        let types = direct_child(node, SyntaxKind::TypeArgumentList)
            .map(|types| self.lower_type_list_container(&types))
            .unwrap_or_default();
        let member = self.lower_name_token(first_name_token(node), node.span());
        let arguments = direct_child(node, SyntaxKind::ArgumentList);
        let count = arguments
            .as_ref()
            .map_or(0, |list| direct_nodes(list).len());
        traversal.push(
            body,
            Step::Call(CallStep::Catalog(
                origin,
                owner,
                types.into_boxed_slice(),
                member,
                count,
            )),
        );
        self.schedule_arguments(body, arguments, traversal);
    }

    pub(super) fn resume_call(&mut self, body: BodyId, step: CallStep, traversal: &mut Traversal) {
        let (origin, kind) = match step {
            CallStep::Callee(node) => {
                let callee = traversal.value();
                let types = direct_child(&node, SyntaxKind::TypeArgumentList)
                    .map(|types| self.lower_type_list_container(&types))
                    .unwrap_or_default();
                let arguments = direct_child(&node, SyntaxKind::ArgumentList);
                let trailing = direct_nodes(&node)
                    .into_iter()
                    .find(|child| child.kind() == SyntaxKind::LambdaExpr);
                let count = arguments
                    .as_ref()
                    .map_or(0, |list| direct_nodes(list).len())
                    + usize::from(trailing.is_some());
                let syntax = if trailing.is_some() {
                    CallSyntax::TrailingArgument
                } else if direct_token(&node, SyntaxKind::ColonColon).is_some() {
                    CallSyntax::ExplicitTypeArguments
                } else {
                    CallSyntax::Direct
                };
                let span = node.span();
                traversal.push(
                    body,
                    Step::Call(CallStep::Finish(
                        node,
                        callee,
                        types.into_boxed_slice(),
                        count,
                        syntax,
                    )),
                );
                if let Some(lambda) = trailing {
                    traversal.push(body, Step::Call(CallStep::TrailingArgument(lambda.span())));
                    traversal.expression(body, Some(lambda), span);
                }
                self.schedule_arguments(body, arguments, traversal);
                return;
            }
            CallStep::Argument(node, fallback) => {
                let origin = self.written(node.span());
                let (name, value, span) = match node.kind() {
                    SyntaxKind::NamedArgument => (
                        Some(self.lower_name_token(first_name_token(&node), node.span())),
                        expression_children(&node).into_iter().next(),
                        node.span(),
                    ),
                    kind if is_expression_kind(kind) => (None, Some(node), fallback),
                    _ => (None, None, node.span()),
                };
                traversal.push(body, Step::Call(CallStep::ArgumentValue(origin, name)));
                traversal.expression(body, value, span);
                return;
            }
            CallStep::ArgumentValue(origin, name) => {
                let value = traversal.value();
                traversal.arguments.push(CallArgument {
                    origin,
                    name,
                    value,
                });
                return;
            }
            CallStep::TrailingArgument(span) => {
                let value = traversal.value();
                let origin = self.derived(span, DesugaringKind::TrailingArgument);
                traversal.arguments.push(CallArgument {
                    name: None,
                    value,
                    origin,
                });
                return;
            }
            CallStep::Finish(node, callee, type_arguments, count, syntax) => {
                let arguments = take_tail(&mut traversal.arguments, count);
                let partial_application = self.call_has_holes(callee, &arguments);
                let origin = if let Some(kind) = syntax.call_desugaring() {
                    self.derived(node.span(), kind)
                } else if partial_application {
                    self.derived(node.span(), DesugaringKind::HolePartialApplication)
                } else {
                    self.written(node.span())
                };
                (
                    origin,
                    ExprKind::Call(CallExpr {
                        callee,
                        arguments,
                        type_arguments,
                        syntax,
                        partial_application,
                    }),
                )
            }
            CallStep::DType(origin, callee, dtype, count) => (
                origin,
                ExprKind::DTypeCall(DTypeCallExpr {
                    callee,
                    dtype,
                    arguments: take_tail(&mut traversal.arguments, count),
                }),
            ),
            CallStep::Catalog(origin, owner, owner_arguments, member, count) => (
                origin,
                ExprKind::CatalogCall(CatalogCallExpr {
                    owner,
                    owner_arguments,
                    member,
                    arguments: take_tail(&mut traversal.arguments, count),
                }),
            ),
        };
        traversal
            .values
            .push(self.alloc_expr(body, Expr::new(origin, kind)));
    }
}

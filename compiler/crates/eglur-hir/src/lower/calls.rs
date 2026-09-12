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

impl Builder {
    pub(super) fn finish_call(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        callee: ExprId,
    ) -> ExprId {
        let type_arguments = direct_child(node, SyntaxKind::TypeArgumentList)
            .map(|types| self.lower_type_list_container(&types))
            .unwrap_or_default();
        let mut arguments = direct_child(node, SyntaxKind::ArgumentList)
            .map(|arguments| self.lower_call_arguments(body, &arguments))
            .unwrap_or_default();
        let trailing = direct_nodes(node)
            .into_iter()
            .find(|child| child.kind() == SyntaxKind::LambdaExpr);
        let syntax = if let Some(lambda) = trailing {
            let lambda_span = lambda.span();
            let value = self.lower_expr(body, Some(lambda), node.span());
            let syntax = CallSyntax::TrailingArgument;
            let argument_origin = self.derived(
                lambda_span,
                syntax
                    .argument_desugaring()
                    .expect("trailing argument syntax is a desugaring"),
            );
            arguments.push(CallArgument {
                name: None,
                value,
                origin: argument_origin,
            });
            syntax
        } else if direct_token(node, SyntaxKind::ColonColon).is_some() {
            CallSyntax::ExplicitTypeArguments
        } else {
            CallSyntax::Direct
        };
        let partial_application = arguments
            .iter()
            .any(|argument| self.expr_is_hole(argument.value));
        let origin = if let Some(kind) = syntax.call_desugaring() {
            self.derived(node.span(), kind)
        } else if partial_application {
            self.derived(node.span(), DesugaringKind::HolePartialApplication)
        } else {
            self.written(node.span())
        };
        self.alloc_expr(
            body,
            Expr::new(
                origin,
                ExprKind::Call(CallExpr {
                    callee,
                    arguments: arguments.into_boxed_slice(),
                    type_arguments: type_arguments.into_boxed_slice(),
                    syntax,
                    partial_application,
                }),
            ),
        )
    }

    pub(super) fn finish_dtype_call(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        callee: ExprId,
    ) -> ExprKind {
        let dtype = direct_child(node, SyntaxKind::TypeArgumentList)
            .and_then(|types| self.lower_type_list_container(&types).into_iter().next())
            .unwrap_or_else(|| self.lower_type(None, node.span()));
        let arguments = direct_child(node, SyntaxKind::ArgumentList)
            .map(|arguments| self.lower_call_arguments(body, &arguments))
            .unwrap_or_default();
        ExprKind::DTypeCall(DTypeCallExpr {
            callee,
            dtype,
            arguments: arguments.into_boxed_slice(),
        })
    }
}

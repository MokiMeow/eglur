use eglur_syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

use super::{
    Builder, direct_child, direct_children, direct_nodes, direct_token, direct_tokens,
    first_name_token, is_expression_kind, is_trivia,
};
use crate::{
    BinaryOperator, CallArgument, CallExpr, CallSyntax, CatalogCallExpr, ConstructorPayload,
    DTypeCallExpr, DesugaringKind, Expr, ExprId, ExprKind, FieldInitializer, FunctionBody, IfExpr,
    InvalidCategory, LambdaExpr, LambdaSyntax, LetSyntax, Literal, LiteralKind, MapEntry, MatchArm,
    MatchExpr, MatchGuard, Mutability, Parameter, Stmt, StmtId, StmtKind,
};

impl Builder {
    pub(super) fn lower_expr(
        &mut self,
        body: crate::BodyId,
        node: Option<SyntaxNode>,
        fallback: eglur_source::FileSpan,
    ) -> ExprId {
        let Some(node) = node else {
            return self.invalid_expr(body, fallback);
        };
        if matches!(
            node.kind(),
            SyntaxKind::BinaryExpr | SyntaxKind::UnaryExpr | SyntaxKind::GroupExpr
        ) {
            return self.lower_operator_tree(body, node);
        }
        if node.kind() == SyntaxKind::CallExpr {
            return self.lower_call(body, &node);
        }

        let written_origin = self.written(node.span());
        let kind = match node.kind() {
            SyntaxKind::BlockExpr => ExprKind::Block {
                statements: direct_nodes(&node)
                    .into_iter()
                    .map(|child| self.lower_stmt(body, Some(child), node.span()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            },
            SyntaxKind::RefExpr => ExprKind::Reference {
                mutability: if direct_token(&node, SyntaxKind::MutKw).is_some() {
                    Mutability::Mutable
                } else {
                    Mutability::Immutable
                },
                value: self.lower_expr(
                    body,
                    expression_children(&node).into_iter().next(),
                    node.span(),
                ),
            },
            SyntaxKind::OwnExpr => {
                ExprKind::Own(self.lower_name_token(first_name_token(&node), node.span()))
            }
            SyntaxKind::MoveExpr => {
                ExprKind::Move(self.lower_name_token(first_name_token(&node), node.span()))
            }
            SyntaxKind::DropOwnExpr => ExprKind::DropOwn {
                explicit_move: direct_token(&node, SyntaxKind::MoveKw).is_some(),
                name: self.lower_name_token(first_name_token(&node), node.span()),
            },
            SyntaxKind::MemberExpr => {
                let receiver = self.lower_expr(
                    body,
                    expression_children(&node).into_iter().next(),
                    node.span(),
                );
                ExprKind::Member {
                    receiver,
                    member: self.lower_name_token(first_name_token(&node), node.span()),
                }
            }
            SyntaxKind::TupleProjectionExpr => {
                let receiver = self.lower_expr(
                    body,
                    expression_children(&node).into_iter().next(),
                    node.span(),
                );
                if let Some(index) =
                    direct_token(&node, SyntaxKind::TupleIndex).filter(|token| !token.is_missing())
                {
                    ExprKind::TupleProjection {
                        receiver,
                        index: index.text().into(),
                    }
                } else {
                    self.invalid.push(crate::InvalidOccurrence::new(
                        InvalidCategory::Expression,
                        written_origin,
                    ));
                    ExprKind::Invalid
                }
            }
            SyntaxKind::TryExpr => ExprKind::Try(self.lower_expr(
                body,
                expression_children(&node).into_iter().next(),
                node.span(),
            )),
            SyntaxKind::DTypeBracketCallExpr => self.lower_dtype_call(body, &node),
            SyntaxKind::ReduceByCandidateExpr => self.lower_reduce_by(body, &node),
            SyntaxKind::LiteralExpr => {
                ExprKind::Literal(self.lower_literal_token(first_literal_token(&node), node.span()))
            }
            SyntaxKind::NameExpr => ExprKind::Name(self.lower_expr_path(Some(&node), node.span())),
            SyntaxKind::HoleExpr => ExprKind::Hole,
            SyntaxKind::UnitExpr => ExprKind::Unit,
            SyntaxKind::UnsafeExpr => ExprKind::Unsafe(self.lower_expr(
                body,
                direct_child(&node, SyntaxKind::BlockExpr),
                node.span(),
            )),
            SyntaxKind::TupleExpr => ExprKind::Tuple(
                expression_children(&node)
                    .into_iter()
                    .map(|child| self.lower_expr(body, Some(child), node.span()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            SyntaxKind::ArrayExpr => ExprKind::Array(
                direct_child(&node, SyntaxKind::ExpressionList)
                    .map(|list| self.lower_expression_list(body, &list))
                    .unwrap_or_default()
                    .into_boxed_slice(),
            ),
            SyntaxKind::MapExpr => ExprKind::Map(
                direct_child(&node, SyntaxKind::MapEntryList)
                    .map(|list| self.lower_map_entries(body, &list))
                    .unwrap_or_default()
                    .into_boxed_slice(),
            ),
            SyntaxKind::StructLiteralExpr => {
                let owner = direct_nodes(&node)
                    .into_iter()
                    .find(|child| child.kind() == SyntaxKind::NameExpr);
                let path = self.lower_expr_path(owner.as_ref(), node.span());
                let fields = direct_child(&node, SyntaxKind::FieldInitializerList)
                    .map(|list| self.lower_field_initializers(body, &list))
                    .unwrap_or_default();
                ExprKind::StructLiteral {
                    path,
                    fields: fields.into_boxed_slice(),
                }
            }
            SyntaxKind::VariantConstructorExpr => self.lower_variant_constructor(body, &node),
            SyntaxKind::LambdaExpr => self.lower_lambda(&node),
            SyntaxKind::IfExpr => self.lower_if(body, &node),
            SyntaxKind::MatchExpr => self.lower_match(body, &node),
            SyntaxKind::ResultExpr => ExprKind::Result(self.lower_expr(
                body,
                direct_child(&node, SyntaxKind::BlockExpr),
                node.span(),
            )),
            SyntaxKind::CheckedExpr => ExprKind::Checked(self.lower_expr(
                body,
                direct_child(&node, SyntaxKind::BlockExpr),
                node.span(),
            )),
            SyntaxKind::TensorSchemaExpr => self.lower_tensor_schema(body, &node, written_origin),
            SyntaxKind::ErrorNode => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::Expression,
                    written_origin,
                ));
                ExprKind::Invalid
            }
            _ => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::Expression,
                    written_origin,
                ));
                ExprKind::Invalid
            }
        };
        self.alloc_expr(body, Expr::new(written_origin, kind))
    }

    pub(super) fn lower_stmt(
        &mut self,
        body: crate::BodyId,
        node: Option<SyntaxNode>,
        fallback: eglur_source::FileSpan,
    ) -> StmtId {
        let Some(node) = node else {
            return self.invalid_stmt(body, fallback);
        };
        let origin = self.written(node.span());
        let kind = match node.kind() {
            SyntaxKind::LetStmt => {
                let pattern =
                    self.lower_pattern(body, direct_child(&node, SyntaxKind::Pattern), node.span());
                let ty = direct_child(&node, SyntaxKind::TypeExpr)
                    .map(|ty| self.lower_type(Some(ty), node.span()));
                let initializer = expression_children(&node)
                    .into_iter()
                    .next()
                    .map(|expr| self.lower_expr(body, Some(expr), node.span()));
                StmtKind::Let {
                    syntax: if direct_token(&node, SyntaxKind::VarKw).is_some() {
                        LetSyntax::Var
                    } else {
                        LetSyntax::Let
                    },
                    mutability: if direct_token(&node, SyntaxKind::MutKw).is_some()
                        || direct_token(&node, SyntaxKind::VarKw).is_some()
                    {
                        Mutability::Mutable
                    } else {
                        Mutability::Immutable
                    },
                    pattern,
                    ty,
                    initializer,
                }
            }
            SyntaxKind::AssignStmt => {
                let mut expressions = expression_children(&node).into_iter();
                StmtKind::Assign {
                    target: self.lower_expr(body, expressions.next(), node.span()),
                    value: self.lower_expr(body, expressions.next(), node.span()),
                }
            }
            SyntaxKind::WhileStmt => {
                let mut expressions = expression_children(&node).into_iter();
                StmtKind::While {
                    condition: self.lower_expr(body, expressions.next(), node.span()),
                    body: self.lower_expr(body, expressions.next(), node.span()),
                }
            }
            SyntaxKind::ForStmt => {
                let mut expressions = expression_children(&node).into_iter();
                StmtKind::For {
                    binding: self.lower_name_token(first_name_token(&node), node.span()),
                    iterable: self.lower_expr(body, expressions.next(), node.span()),
                    body: self.lower_expr(body, expressions.next(), node.span()),
                }
            }
            SyntaxKind::BreakStmt => StmtKind::Break,
            SyntaxKind::ContinueStmt => StmtKind::Continue,
            SyntaxKind::ReturnStmt => StmtKind::Return(
                expression_children(&node)
                    .into_iter()
                    .next()
                    .map(|expr| self.lower_expr(body, Some(expr), node.span())),
            ),
            SyntaxKind::ExprStmt => StmtKind::Expression(self.lower_expr(
                body,
                expression_children(&node).into_iter().next(),
                node.span(),
            )),
            SyntaxKind::YieldStmt => StmtKind::Yield(self.lower_expr(
                body,
                expression_children(&node).into_iter().next(),
                node.span(),
            )),
            SyntaxKind::ErrorNode => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::Statement,
                    origin,
                ));
                StmtKind::Invalid
            }
            _ => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::Statement,
                    origin,
                ));
                StmtKind::Invalid
            }
        };
        self.alloc_stmt(body, Stmt::new(origin, kind))
    }

    pub(super) fn lower_function_body(
        &mut self,
        body: crate::BodyId,
        declaration: &SyntaxNode,
    ) -> FunctionBody {
        let Some(expression) = expression_children(declaration).into_iter().next() else {
            return FunctionBody::Absent;
        };
        let block = expression.kind() == SyntaxKind::BlockExpr;
        let expression = self.lower_expr(body, Some(expression), declaration.span());
        if block {
            FunctionBody::Block(expression)
        } else {
            FunctionBody::Expression(expression)
        }
    }

    pub(super) fn lower_literal_token(
        &mut self,
        token: Option<SyntaxToken>,
        fallback: eglur_source::FileSpan,
    ) -> Literal {
        let Some(token) = token.filter(|token| !token.is_missing()) else {
            let origin = self.record_invalid(InvalidCategory::Literal, fallback);
            return Literal::new(LiteralKind::Invalid, "".into(), origin);
        };
        let kind = match token.kind() {
            SyntaxKind::Integer => LiteralKind::Integer,
            SyntaxKind::Float => LiteralKind::Float,
            SyntaxKind::String => LiteralKind::String,
            SyntaxKind::FormatString => LiteralKind::FormatString,
            SyntaxKind::TrueKw | SyntaxKind::FalseKw => LiteralKind::Bool,
            SyntaxKind::AxisName => LiteralKind::AxisName,
            SyntaxKind::AxisIndex => LiteralKind::AxisIndex,
            _ => {
                let origin = self.record_invalid(InvalidCategory::Literal, token.span());
                return Literal::new(LiteralKind::Invalid, token.text().into(), origin);
            }
        };
        Literal::new(kind, token.text().into(), self.written(token.span()))
    }

    fn lower_call(&mut self, body: crate::BodyId, node: &SyntaxNode) -> ExprId {
        if self.is_catalog_call(node) {
            let origin = self.written(node.span());
            let kind = ExprKind::CatalogCall(self.lower_catalog_call(body, node));
            return self.alloc_expr(body, Expr::new(origin, kind));
        }
        if let Some((syntax, callee, inputs)) = self.lower_postfix_application(body, node) {
            let origin = self.derived(
                node.span(),
                syntax
                    .call_desugaring()
                    .expect("postfix application is a desugaring"),
            );
            let partial_application = self.expr_is_hole(callee)
                || inputs
                    .iter()
                    .any(|argument| self.expr_is_hole(argument.value));
            let kind = ExprKind::Call(CallExpr {
                callee,
                arguments: inputs.into_boxed_slice(),
                type_arguments: Box::new([]),
                syntax,
                partial_application,
            });
            return self.alloc_expr(body, Expr::new(origin, kind));
        }

        let expression_nodes = expression_children(node);
        let callee = self.lower_expr(body, expression_nodes.first().cloned(), node.span());
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

    fn lower_postfix_application(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
    ) -> Option<(CallSyntax, ExprId, Vec<CallArgument>)> {
        let callee_node = expression_children(node).into_iter().next()?;
        if callee_node.kind() != SyntaxKind::MemberExpr
            || direct_child(node, SyntaxKind::TypeArgumentList).is_some()
        {
            return None;
        }
        let member = first_name_token(&callee_node)?;
        let syntax = match member.text() {
            "then" => CallSyntax::PostfixThen,
            "apply" => CallSyntax::PostfixApply,
            _ => return None,
        };
        let argument_list = direct_child(node, SyntaxKind::ArgumentList)?;
        let argument_nodes = direct_nodes(&argument_list);
        if argument_nodes.len() != 1 || argument_nodes[0].kind() == SyntaxKind::NamedArgument {
            return None;
        }
        let callable_node = argument_nodes[0].clone();
        if !is_expression_kind(callable_node.kind()) {
            return None;
        }
        let receiver_node = expression_children(&callee_node).into_iter().next()?;
        let callable = self.lower_expr(body, Some(callable_node), node.span());
        let input_nodes = if syntax == CallSyntax::PostfixApply
            && receiver_node.kind() == SyntaxKind::TupleExpr
        {
            expression_children(&receiver_node)
        } else if syntax == CallSyntax::PostfixThen {
            vec![receiver_node]
        } else {
            return None;
        };
        let inputs = input_nodes
            .into_iter()
            .map(|input| {
                let span = input.span();
                CallArgument {
                    name: None,
                    value: self.lower_expr(body, Some(input), node.span()),
                    origin: self.written(span),
                }
            })
            .collect();
        Some((syntax, callable, inputs))
    }

    fn is_catalog_call(&self, node: &SyntaxNode) -> bool {
        direct_token(node, SyntaxKind::Dot).is_some()
            && direct_child(node, SyntaxKind::TypeArgumentList).is_some()
            && direct_token(node, SyntaxKind::ColonColon).is_none()
    }

    fn lower_catalog_call(&mut self, body: crate::BodyId, node: &SyntaxNode) -> CatalogCallExpr {
        let owner_node = expression_children(node).into_iter().next();
        let owner = self.lower_expr_path(owner_node.as_ref(), node.span());
        let owner_arguments = direct_child(node, SyntaxKind::TypeArgumentList)
            .map(|types| self.lower_type_list_container(&types))
            .unwrap_or_default();
        let member = self.lower_name_token(first_name_token(node), node.span());
        let arguments = direct_child(node, SyntaxKind::ArgumentList)
            .map(|arguments| self.lower_call_arguments(body, &arguments))
            .unwrap_or_default();
        CatalogCallExpr {
            owner,
            owner_arguments: owner_arguments.into_boxed_slice(),
            member,
            arguments: arguments.into_boxed_slice(),
        }
    }

    pub(super) fn lower_call_arguments(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
    ) -> Vec<CallArgument> {
        direct_nodes(node)
            .into_iter()
            .map(|child| match child.kind() {
                SyntaxKind::NamedArgument => {
                    let origin = self.written(child.span());
                    let name = self.lower_name_token(first_name_token(&child), child.span());
                    let value = self.lower_expr(
                        body,
                        expression_children(&child).into_iter().next(),
                        child.span(),
                    );
                    CallArgument {
                        name: Some(name),
                        value,
                        origin,
                    }
                }
                kind if is_expression_kind(kind) => {
                    let origin = self.written(child.span());
                    let value = self.lower_expr(body, Some(child), node.span());
                    CallArgument {
                        name: None,
                        value,
                        origin,
                    }
                }
                _ => {
                    let origin = self.written(child.span());
                    let value = self.invalid_expr(body, child.span());
                    CallArgument {
                        name: None,
                        value,
                        origin,
                    }
                }
            })
            .collect()
    }

    fn lower_dtype_call(&mut self, body: crate::BodyId, node: &SyntaxNode) -> ExprKind {
        let callee = self.lower_expr(
            body,
            expression_children(node).into_iter().next(),
            node.span(),
        );
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

    fn lower_expression_list(&mut self, body: crate::BodyId, node: &SyntaxNode) -> Vec<ExprId> {
        expression_children(node)
            .into_iter()
            .map(|child| self.lower_expr(body, Some(child), node.span()))
            .collect()
    }

    fn lower_map_entries(&mut self, body: crate::BodyId, node: &SyntaxNode) -> Vec<MapEntry> {
        direct_children(node, SyntaxKind::MapEntry)
            .into_iter()
            .map(|entry| {
                let origin = self.written(entry.span());
                let mut expressions = expression_children(&entry).into_iter();
                MapEntry {
                    key: self.lower_expr(body, expressions.next(), entry.span()),
                    value: self.lower_expr(body, expressions.next(), entry.span()),
                    origin,
                }
            })
            .collect()
    }

    fn lower_field_initializers(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
    ) -> Vec<FieldInitializer> {
        direct_children(node, SyntaxKind::FieldInitializer)
            .into_iter()
            .map(|field| {
                let origin = self.written(field.span());
                FieldInitializer {
                    name: self.lower_name_token(first_name_token(&field), field.span()),
                    value: self.lower_expr(
                        body,
                        expression_children(&field).into_iter().next(),
                        field.span(),
                    ),
                    origin,
                }
            })
            .collect()
    }

    fn lower_variant_constructor(&mut self, body: crate::BodyId, node: &SyntaxNode) -> ExprKind {
        let owner = direct_nodes(node)
            .into_iter()
            .find(|child| child.kind() == SyntaxKind::NameExpr);
        let path = self.lower_expr_path(owner.as_ref(), node.span());
        let payload = if let Some(list) = direct_child(node, SyntaxKind::ExpressionList) {
            ConstructorPayload::Tuple(self.lower_expression_list(body, &list).into_boxed_slice())
        } else if let Some(list) = direct_child(node, SyntaxKind::FieldInitializerList) {
            ConstructorPayload::Named(
                self.lower_field_initializers(body, &list)
                    .into_boxed_slice(),
            )
        } else if direct_token(node, SyntaxKind::LeftParen).is_some() {
            ConstructorPayload::Tuple(Box::new([]))
        } else if direct_token(node, SyntaxKind::LeftBrace).is_some() {
            ConstructorPayload::Named(Box::new([]))
        } else {
            ConstructorPayload::Absent
        };
        ExprKind::VariantConstructor { path, payload }
    }

    fn lower_expr_path(
        &mut self,
        node: Option<&SyntaxNode>,
        fallback: eglur_source::FileSpan,
    ) -> crate::Path {
        let Some(node) = node else {
            return self.lower_path(None, fallback);
        };
        if let Some(path) = direct_child(node, SyntaxKind::Path) {
            return self.lower_path(Some(path), fallback);
        }
        let name = self.lower_name_token(first_name_token(node), node.span());
        let origin = name.origin();
        crate::Path::new(vec![name].into_boxed_slice(), origin)
    }

    fn lower_lambda(&mut self, node: &SyntaxNode) -> ExprKind {
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
                    .map(|stmt| self.lower_stmt(body, Some(stmt), lambda_body.span()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let tail = lambda_body.as_ref().and_then(|lambda_body| {
            expression_children(lambda_body)
                .into_iter()
                .last()
                .map(|expr| self.lower_expr(body, Some(expr), lambda_body.span()))
        });
        let syntax = if explicit {
            LambdaSyntax::Explicit
        } else {
            LambdaSyntax::Implicit
        };
        ExprKind::Lambda(LambdaExpr {
            syntax,
            body,
            parameters: parameters.into_boxed_slice(),
            statements: statements.into_boxed_slice(),
            tail,
        })
    }

    fn lower_if(&mut self, body: crate::BodyId, node: &SyntaxNode) -> ExprKind {
        let expressions = expression_children(node);
        let condition = self.lower_expr(body, expressions.first().cloned(), node.span());
        let then_branch = self.lower_expr(body, expressions.get(1).cloned(), node.span());
        let remaining = &expressions[expressions.len().min(2)..];
        let (else_branch, unknown_branch) = if remaining.len() >= 2 {
            (
                Some(self.lower_expr(body, remaining.first().cloned(), node.span())),
                Some(self.lower_expr(body, remaining.get(1).cloned(), node.span())),
            )
        } else {
            (
                remaining
                    .first()
                    .cloned()
                    .map(|expr| self.lower_expr(body, Some(expr), node.span())),
                None,
            )
        };
        ExprKind::If(IfExpr {
            condition,
            then_branch,
            else_branch,
            unknown_branch,
        })
    }

    fn lower_match(&mut self, body: crate::BodyId, node: &SyntaxNode) -> ExprKind {
        let scrutinee = self.lower_expr(
            body,
            expression_children(node).into_iter().next(),
            node.span(),
        );
        let arms = direct_children(node, SyntaxKind::MatchArm)
            .into_iter()
            .map(|arm| {
                let origin = self.written(arm.span());
                let pattern =
                    self.lower_pattern(body, direct_child(&arm, SyntaxKind::Pattern), arm.span());
                let guard = direct_child(&arm, SyntaxKind::MatchGuard).map(|guard| {
                    let guard_origin = self.written(guard.span());
                    MatchGuard {
                        condition: self.lower_expr(
                            body,
                            expression_children(&guard).into_iter().next(),
                            guard.span(),
                        ),
                        origin: guard_origin,
                    }
                });
                let value = self.lower_expr(
                    body,
                    expression_children(&arm).into_iter().last(),
                    arm.span(),
                );
                MatchArm {
                    pattern,
                    guard,
                    value,
                    origin,
                }
            })
            .collect::<Vec<_>>();
        ExprKind::Match(MatchExpr {
            scrutinee,
            arms: arms.into_boxed_slice(),
        })
    }

    pub(super) fn expr_is_hole(&self, id: ExprId) -> bool {
        matches!(
            self.bodies[id.body().index()].expressions[id.index()].kind(),
            ExprKind::Hole
        )
    }

    fn invalid_expr(&mut self, body: crate::BodyId, span: eglur_source::FileSpan) -> ExprId {
        let origin = self.record_invalid(InvalidCategory::Expression, span);
        self.alloc_expr(body, Expr::new(origin, ExprKind::Invalid))
    }

    fn invalid_stmt(&mut self, body: crate::BodyId, span: eglur_source::FileSpan) -> StmtId {
        let origin = self.record_invalid(InvalidCategory::Statement, span);
        self.alloc_stmt(body, Stmt::new(origin, StmtKind::Invalid))
    }
}

pub(super) fn expression_children(node: &SyntaxNode) -> Vec<SyntaxNode> {
    direct_nodes(node)
        .into_iter()
        .filter(|child| is_expression_kind(child.kind()))
        .collect()
}

fn first_literal_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .find_map(|element| match element {
            SyntaxElement::Token(token) if !is_trivia(token.kind()) => Some(token),
            SyntaxElement::Node(_) | SyntaxElement::Token(_) => None,
        })
}

pub(super) fn lower_binary_operator(node: &SyntaxNode) -> Option<BinaryOperator> {
    direct_tokens(node)
        .into_iter()
        .find_map(|token| match token.kind() {
            SyntaxKind::LogicalOr => Some(BinaryOperator::LogicalOr),
            SyntaxKind::LogicalAnd => Some(BinaryOperator::LogicalAnd),
            SyntaxKind::EqualEqual => Some(BinaryOperator::Equal),
            SyntaxKind::NotEqual => Some(BinaryOperator::NotEqual),
            SyntaxKind::Less => Some(BinaryOperator::Less),
            SyntaxKind::LessEqual => Some(BinaryOperator::LessEqual),
            SyntaxKind::Greater => Some(BinaryOperator::Greater),
            SyntaxKind::GreaterEqual => Some(BinaryOperator::GreaterEqual),
            SyntaxKind::Range => Some(BinaryOperator::Range),
            SyntaxKind::RangeInclusive => Some(BinaryOperator::RangeInclusive),
            SyntaxKind::Plus => Some(BinaryOperator::Add),
            SyntaxKind::Minus => Some(BinaryOperator::Subtract),
            SyntaxKind::Star => Some(BinaryOperator::Multiply),
            SyntaxKind::Slash => Some(BinaryOperator::Divide),
            SyntaxKind::FractionalDivide => Some(BinaryOperator::FractionalDivide),
            SyntaxKind::Percent => Some(BinaryOperator::Remainder),
            _ => None,
        })
}

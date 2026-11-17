use eglur_syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

use super::traversal::{Step, Traversal, take_tail};
use super::{
    Builder, direct_child, direct_nodes, direct_token, direct_tokens, first_name_token,
    is_expression_kind, is_trivia,
};
use crate::{
    BinaryOperator, BodyId, Expr, ExprId, ExprKind, FunctionBody, InvalidCategory, LetSyntax,
    Literal, LiteralKind, Mutability, OriginId, PatternId, Stmt, StmtKind, TypeRefId,
};

pub(super) enum BodyStep {
    Statement(SyntaxNode),
    FinishStatement(OriginId, StatementFinish),
    Block(OriginId, usize),
    Wrapper(OriginId, Wrapper),
}

pub(super) enum Wrapper {
    Unsafe,
    Result,
    Checked,
}

pub(super) enum StatementFinish {
    Let {
        syntax: LetSyntax,
        mutability: Mutability,
        pattern: PatternId,
        ty: Option<TypeRefId>,
        initializer: bool,
    },
    Assign,
    While,
    For(crate::Name),
    Return(bool),
    Expression,
    Yield,
}

impl Builder {
    pub(super) fn enter_body_expr(
        &mut self,
        body: BodyId,
        node: SyntaxNode,
        traversal: &mut Traversal,
    ) {
        let origin = self.written(node.span());
        let kind = match node.kind() {
            SyntaxKind::CallExpr => {
                self.schedule_catalog_call(body, origin, &node, traversal);
                return;
            }
            SyntaxKind::BlockExpr => {
                let statements = direct_nodes(&node);
                traversal.push(body, Step::Body(BodyStep::Block(origin, statements.len())));
                for statement in statements.into_iter().rev() {
                    traversal.push(body, Step::Body(BodyStep::Statement(statement)));
                }
                return;
            }
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
            SyntaxKind::LiteralExpr => {
                ExprKind::Literal(self.lower_literal_token(first_literal_token(&node), node.span()))
            }
            SyntaxKind::NameExpr => ExprKind::Name(self.lower_expr_path(Some(&node), node.span())),
            SyntaxKind::HoleExpr => ExprKind::Hole,
            SyntaxKind::UnitExpr => ExprKind::Unit,
            SyntaxKind::UnsafeExpr | SyntaxKind::ResultExpr | SyntaxKind::CheckedExpr => {
                let wrapper = match node.kind() {
                    SyntaxKind::UnsafeExpr => Wrapper::Unsafe,
                    SyntaxKind::ResultExpr => Wrapper::Result,
                    _ => Wrapper::Checked,
                };
                traversal.push(body, Step::Body(BodyStep::Wrapper(origin, wrapper)));
                traversal.expression(
                    body,
                    direct_child(&node, SyntaxKind::BlockExpr),
                    node.span(),
                );
                return;
            }
            SyntaxKind::TupleExpr
            | SyntaxKind::ArrayExpr
            | SyntaxKind::MapExpr
            | SyntaxKind::StructLiteralExpr
            | SyntaxKind::VariantConstructorExpr => {
                self.schedule_aggregate(body, origin, &node, traversal);
                return;
            }
            SyntaxKind::LambdaExpr | SyntaxKind::IfExpr | SyntaxKind::MatchExpr => {
                self.schedule_control(body, origin, &node, traversal);
                return;
            }
            SyntaxKind::TensorSchemaExpr => self.lower_tensor_schema(body, &node, origin),
            _ => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::Expression,
                    origin,
                ));
                ExprKind::Invalid
            }
        };
        traversal
            .values
            .push(self.alloc_expr(body, Expr::new(origin, kind)));
    }

    pub(super) fn resume_body(&mut self, body: BodyId, step: BodyStep, traversal: &mut Traversal) {
        match step {
            BodyStep::Statement(node) => self.schedule_statement(body, node, traversal),
            BodyStep::FinishStatement(origin, finish) => {
                let kind = match finish {
                    StatementFinish::Let {
                        syntax,
                        mutability,
                        pattern,
                        ty,
                        initializer,
                    } => StmtKind::Let {
                        syntax,
                        mutability,
                        pattern,
                        ty,
                        initializer: initializer.then(|| traversal.value()),
                    },
                    StatementFinish::Assign => {
                        let value = traversal.value();
                        StmtKind::Assign {
                            target: traversal.value(),
                            value,
                        }
                    }
                    StatementFinish::While => {
                        let loop_body = traversal.value();
                        StmtKind::While {
                            condition: traversal.value(),
                            body: loop_body,
                        }
                    }
                    StatementFinish::For(binding) => {
                        let loop_body = traversal.value();
                        StmtKind::For {
                            binding,
                            iterable: traversal.value(),
                            body: loop_body,
                        }
                    }
                    StatementFinish::Return(value) => {
                        StmtKind::Return(value.then(|| traversal.value()))
                    }
                    StatementFinish::Expression => StmtKind::Expression(traversal.value()),
                    StatementFinish::Yield => StmtKind::Yield(traversal.value()),
                };
                traversal
                    .statements
                    .push(self.alloc_stmt(body, Stmt::new(origin, kind)));
            }
            BodyStep::Block(origin, count) => {
                let statements = take_tail(&mut traversal.statements, count);
                traversal
                    .values
                    .push(self.alloc_expr(body, Expr::new(origin, ExprKind::Block { statements })));
            }
            BodyStep::Wrapper(origin, wrapper) => {
                let value = traversal.value();
                let kind = match wrapper {
                    Wrapper::Unsafe => ExprKind::Unsafe(value),
                    Wrapper::Result => ExprKind::Result(value),
                    Wrapper::Checked => ExprKind::Checked(value),
                };
                traversal
                    .values
                    .push(self.alloc_expr(body, Expr::new(origin, kind)));
            }
        }
    }

    fn schedule_statement(&mut self, body: BodyId, node: SyntaxNode, traversal: &mut Traversal) {
        let origin = self.written(node.span());
        let mut expressions = expression_children(&node).into_iter();
        let (finish, children) = match node.kind() {
            SyntaxKind::LetStmt => {
                let pattern =
                    self.lower_pattern(body, direct_child(&node, SyntaxKind::Pattern), node.span());
                let ty = direct_child(&node, SyntaxKind::TypeExpr)
                    .map(|ty| self.lower_type(Some(ty), node.span()));
                let initializer = expressions.next();
                let finish = StatementFinish::Let {
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
                    initializer: initializer.is_some(),
                };
                (
                    finish,
                    initializer.map(Some).into_iter().collect::<Vec<_>>(),
                )
            }
            SyntaxKind::AssignStmt => (
                StatementFinish::Assign,
                vec![expressions.next(), expressions.next()],
            ),
            SyntaxKind::WhileStmt => (
                StatementFinish::While,
                vec![expressions.next(), expressions.next()],
            ),
            SyntaxKind::ForStmt => (
                StatementFinish::For(self.lower_name_token(first_name_token(&node), node.span())),
                vec![expressions.next(), expressions.next()],
            ),
            SyntaxKind::ReturnStmt => {
                let value = expressions.next();
                (
                    StatementFinish::Return(value.is_some()),
                    value.map(Some).into_iter().collect(),
                )
            }
            SyntaxKind::ExprStmt => (StatementFinish::Expression, vec![expressions.next()]),
            SyntaxKind::YieldStmt => (StatementFinish::Yield, vec![expressions.next()]),
            _ => {
                let kind = match node.kind() {
                    SyntaxKind::BreakStmt => StmtKind::Break,
                    SyntaxKind::ContinueStmt => StmtKind::Continue,
                    _ => {
                        self.invalid.push(crate::InvalidOccurrence::new(
                            InvalidCategory::Statement,
                            origin,
                        ));
                        StmtKind::Invalid
                    }
                };
                traversal
                    .statements
                    .push(self.alloc_stmt(body, Stmt::new(origin, kind)));
                return;
            }
        };
        traversal.push(body, Step::Body(BodyStep::FinishStatement(origin, finish)));
        for child in children.into_iter().rev() {
            traversal.expression(body, child, node.span());
        }
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

    pub(super) fn is_catalog_call(&self, node: &SyntaxNode) -> bool {
        direct_token(node, SyntaxKind::Dot).is_some()
            && direct_child(node, SyntaxKind::TypeArgumentList).is_some()
            && direct_token(node, SyntaxKind::ColonColon).is_none()
    }

    pub(super) fn lower_expr_path(
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

    pub(super) fn expr_is_hole(&self, id: ExprId) -> bool {
        matches!(
            self.bodies[id.body().index()].expressions[id.index()].kind(),
            ExprKind::Hole
        )
    }

    pub(super) fn invalid_expr(
        &mut self,
        body: crate::BodyId,
        span: eglur_source::FileSpan,
    ) -> ExprId {
        let origin = self.record_invalid(InvalidCategory::Expression, span);
        self.alloc_expr(body, Expr::new(origin, ExprKind::Invalid))
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

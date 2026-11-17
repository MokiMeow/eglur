use eglur_syntax::{SyntaxKind, SyntaxNode};

use super::body::expression_children;
use super::traversal::{Step, Traversal, take_tail};
use super::{Builder, direct_child, direct_children, first_name_token};
use crate::{
    BodyId, ConstructorPayload, Expr, ExprKind, FieldInitializer, MapEntry, Name, OriginId, Path,
};

pub(super) enum AggregateKind {
    Tuple,
    Array,
    Map,
    Struct(Path),
    VariantTuple(Path),
    VariantNamed(Path),
}

pub(super) enum AggregateStep {
    Finish(OriginId, AggregateKind, usize),
    Field(SyntaxNode),
    FieldValue(OriginId, Name),
    Entry(SyntaxNode),
    EntryValues(OriginId),
}

impl Builder {
    pub(super) fn schedule_aggregate(
        &mut self,
        body: BodyId,
        origin: OriginId,
        node: &SyntaxNode,
        traversal: &mut Traversal,
    ) {
        let (kind, children) = match node.kind() {
            SyntaxKind::TupleExpr => (AggregateKind::Tuple, expression_children(node)),
            SyntaxKind::ArrayExpr => (
                AggregateKind::Array,
                direct_child(node, SyntaxKind::ExpressionList)
                    .map(|list| expression_children(&list))
                    .unwrap_or_default(),
            ),
            SyntaxKind::MapExpr => (
                AggregateKind::Map,
                direct_child(node, SyntaxKind::MapEntryList)
                    .map(|list| direct_children(&list, SyntaxKind::MapEntry))
                    .unwrap_or_default(),
            ),
            SyntaxKind::StructLiteralExpr => {
                let owner = expression_children(node)
                    .into_iter()
                    .find(|child| child.kind() == SyntaxKind::NameExpr);
                let path = self.lower_expr_path(owner.as_ref(), node.span());
                let fields = direct_child(node, SyntaxKind::FieldInitializerList)
                    .map(|list| direct_children(&list, SyntaxKind::FieldInitializer))
                    .unwrap_or_default();
                (AggregateKind::Struct(path), fields)
            }
            SyntaxKind::VariantConstructorExpr => {
                let owner = expression_children(node)
                    .into_iter()
                    .find(|child| child.kind() == SyntaxKind::NameExpr);
                let path = self.lower_expr_path(owner.as_ref(), node.span());
                if let Some(list) = direct_child(node, SyntaxKind::ExpressionList) {
                    (
                        AggregateKind::VariantTuple(path),
                        expression_children(&list),
                    )
                } else if let Some(list) = direct_child(node, SyntaxKind::FieldInitializerList) {
                    (
                        AggregateKind::VariantNamed(path),
                        direct_children(&list, SyntaxKind::FieldInitializer),
                    )
                } else {
                    let payload = if super::direct_token(node, SyntaxKind::LeftParen).is_some() {
                        ConstructorPayload::Tuple(Box::new([]))
                    } else if super::direct_token(node, SyntaxKind::LeftBrace).is_some() {
                        ConstructorPayload::Named(Box::new([]))
                    } else {
                        ConstructorPayload::Absent
                    };
                    traversal.values.push(self.alloc_expr(
                        body,
                        Expr::new(origin, ExprKind::VariantConstructor { path, payload }),
                    ));
                    return;
                }
            }
            _ => unreachable!("aggregate syntax was classified at expression entry"),
        };
        let fields = matches!(
            kind,
            AggregateKind::Struct(_) | AggregateKind::VariantNamed(_)
        );
        let entries = matches!(kind, AggregateKind::Map);
        traversal.push(
            body,
            Step::Aggregate(AggregateStep::Finish(origin, kind, children.len())),
        );
        for child in children.into_iter().rev() {
            if fields {
                traversal.push(body, Step::Aggregate(AggregateStep::Field(child)));
            } else if entries {
                traversal.push(body, Step::Aggregate(AggregateStep::Entry(child)));
            } else {
                traversal.expression(body, Some(child), node.span());
            }
        }
    }

    pub(super) fn resume_aggregate(
        &mut self,
        body: BodyId,
        step: AggregateStep,
        traversal: &mut Traversal,
    ) {
        let (origin, kind) = match step {
            AggregateStep::Field(node) => {
                let origin = self.written(node.span());
                let name = self.lower_name_token(first_name_token(&node), node.span());
                traversal.push(
                    body,
                    Step::Aggregate(AggregateStep::FieldValue(origin, name)),
                );
                traversal.expression(
                    body,
                    expression_children(&node).into_iter().next(),
                    node.span(),
                );
                return;
            }
            AggregateStep::FieldValue(origin, name) => {
                let value = traversal.value();
                traversal.fields.push(FieldInitializer {
                    name,
                    value,
                    origin,
                });
                return;
            }
            AggregateStep::Entry(node) => {
                let origin = self.written(node.span());
                let mut children = expression_children(&node).into_iter();
                let key = children.next();
                let value = children.next();
                traversal.push(body, Step::Aggregate(AggregateStep::EntryValues(origin)));
                traversal.expression(body, value, node.span());
                traversal.expression(body, key, node.span());
                return;
            }
            AggregateStep::EntryValues(origin) => {
                let value = traversal.value();
                let key = traversal.value();
                traversal.entries.push(MapEntry { key, value, origin });
                return;
            }
            AggregateStep::Finish(origin, kind, count) => {
                let kind = match kind {
                    AggregateKind::Tuple => {
                        ExprKind::Tuple(take_tail(&mut traversal.values, count))
                    }
                    AggregateKind::Array => {
                        ExprKind::Array(take_tail(&mut traversal.values, count))
                    }
                    AggregateKind::Map => ExprKind::Map(take_tail(&mut traversal.entries, count)),
                    AggregateKind::Struct(path) => ExprKind::StructLiteral {
                        path,
                        fields: take_tail(&mut traversal.fields, count),
                    },
                    AggregateKind::VariantTuple(path) => ExprKind::VariantConstructor {
                        path,
                        payload: ConstructorPayload::Tuple(take_tail(&mut traversal.values, count)),
                    },
                    AggregateKind::VariantNamed(path) => ExprKind::VariantConstructor {
                        path,
                        payload: ConstructorPayload::Named(take_tail(&mut traversal.fields, count)),
                    },
                };
                (origin, kind)
            }
        };
        traversal
            .values
            .push(self.alloc_expr(body, Expr::new(origin, kind)));
    }
}

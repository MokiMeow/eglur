use eglur_syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

use super::{
    Builder, direct_child, direct_nodes, direct_token, first_name_token, is_pattern_kind, is_trivia,
};
use crate::{
    FieldPattern, InvalidCategory, Pattern, PatternElement, PatternId, PatternKind, PatternPayload,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum PatternContext {
    Binding,
    Match,
}

impl Builder {
    pub(super) fn lower_pattern(
        &mut self,
        body: crate::BodyId,
        node: Option<SyntaxNode>,
        fallback: eglur_source::FileSpan,
        context: PatternContext,
    ) -> PatternId {
        let Some(node) = node else {
            return self.invalid_pattern(body, fallback);
        };
        let origin = self.written(node.span());
        let kind = match node.kind() {
            SyntaxKind::Pattern => {
                let nested = direct_nodes(&node)
                    .into_iter()
                    .find(|child| is_pattern_kind(child.kind()));
                return self.lower_pattern(body, nested, node.span(), context);
            }
            SyntaxKind::OrPattern => PatternKind::Or(
                direct_nodes(&node)
                    .into_iter()
                    .map(|child| self.lower_pattern(body, Some(child), node.span(), context))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            SyntaxKind::AliasPattern => {
                let path = self.lower_path(direct_child(&node, SyntaxKind::Path), node.span());
                let name = path
                    .segments()
                    .first()
                    .cloned()
                    .unwrap_or_else(|| self.invalid_name(node.span()));
                let nested = direct_nodes(&node).into_iter().find(|child| {
                    is_pattern_kind(child.kind()) && child.kind() != SyntaxKind::Path
                });
                let pattern = self.lower_pattern(body, nested, node.span(), context);
                PatternKind::Alias { name, pattern }
            }
            SyntaxKind::BindingPattern => {
                let path = self.lower_path(direct_child(&node, SyntaxKind::Path), node.span());
                if context == PatternContext::Match && path.is_unqualified_residual_constructor() {
                    PatternKind::Variant {
                        path,
                        payload: PatternPayload::Absent,
                    }
                } else {
                    PatternKind::Binding(
                        path.segments()
                            .first()
                            .cloned()
                            .unwrap_or_else(|| self.invalid_name(node.span())),
                    )
                }
            }
            SyntaxKind::WildcardPattern => PatternKind::Wildcard,
            SyntaxKind::GroupPattern => {
                let nested = direct_nodes(&node)
                    .into_iter()
                    .find(|child| child.kind() == SyntaxKind::Pattern);
                PatternKind::Group(self.lower_pattern(body, nested, node.span(), context))
            }
            SyntaxKind::LiteralPattern => {
                let literal = self.lower_literal_token(
                    node.children_with_tokens()
                        .find_map(|element| match element {
                            SyntaxElement::Token(token) if !is_trivia(token.kind()) => Some(token),
                            SyntaxElement::Node(_) | SyntaxElement::Token(_) => None,
                        }),
                    node.span(),
                );
                PatternKind::Literal(literal)
            }
            SyntaxKind::UnitPattern => PatternKind::Unit,
            SyntaxKind::VariantPattern => self.lower_variant_pattern(body, &node, context),
            SyntaxKind::TuplePattern => PatternKind::Tuple(
                self.lower_pattern_elements(body, &node, context)
                    .into_boxed_slice(),
            ),
            SyntaxKind::StructPattern => {
                let path = direct_child(&node, SyntaxKind::Path)
                    .map(|path| self.lower_path(Some(path), node.span()));
                let fields = direct_child(&node, SyntaxKind::FieldPatternList)
                    .map(|list| self.lower_field_patterns(body, &list, context))
                    .unwrap_or_default();
                match path {
                    Some(path)
                        if context == PatternContext::Match
                            && path.is_unqualified_residual_constructor() =>
                    {
                        PatternKind::Variant {
                            path,
                            payload: PatternPayload::Named(fields.into_boxed_slice()),
                        }
                    }
                    path => PatternKind::Struct {
                        path,
                        fields: fields.into_boxed_slice(),
                    },
                }
            }
            SyntaxKind::ErrorNode => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::Pattern,
                    origin,
                ));
                PatternKind::Invalid
            }
            _ => {
                self.invalid.push(crate::InvalidOccurrence::new(
                    InvalidCategory::Pattern,
                    origin,
                ));
                PatternKind::Invalid
            }
        };
        self.alloc_pattern(body, Pattern::new(origin, kind))
    }

    pub(super) fn lower_binding_pattern_from_token(
        &mut self,
        body: crate::BodyId,
        token: Option<eglur_syntax::SyntaxToken>,
        fallback: eglur_source::FileSpan,
    ) -> PatternId {
        let origin = match token.as_ref() {
            Some(token) => self.written(token.span()),
            None => self.record_invalid(InvalidCategory::Pattern, fallback),
        };
        let kind = match token {
            Some(token) if token.kind() == SyntaxKind::Underscore => PatternKind::Wildcard,
            Some(token) => PatternKind::Binding(self.lower_name_token(Some(token), fallback)),
            None => PatternKind::Invalid,
        };
        self.alloc_pattern(body, Pattern::new(origin, kind))
    }

    fn lower_variant_pattern(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        context: PatternContext,
    ) -> PatternKind {
        let path = self.lower_path(direct_child(node, SyntaxKind::Path), node.span());
        if let Some(list) = direct_child(node, SyntaxKind::PatternList) {
            return PatternKind::Variant {
                path,
                payload: PatternPayload::Positional(
                    self.lower_pattern_elements(body, &list, context)
                        .into_boxed_slice(),
                ),
            };
        }
        if let Some(list) = direct_child(node, SyntaxKind::FieldPatternList) {
            return PatternKind::Variant {
                path,
                payload: PatternPayload::Named(
                    self.lower_field_patterns(body, &list, context)
                        .into_boxed_slice(),
                ),
            };
        }
        let payload = if direct_token(node, SyntaxKind::LeftParen).is_some() {
            PatternPayload::Positional(Box::new([]))
        } else if direct_token(node, SyntaxKind::LeftBrace).is_some() {
            PatternPayload::Named(Box::new([]))
        } else {
            PatternPayload::Absent
        };
        PatternKind::Variant { path, payload }
    }

    fn lower_pattern_elements(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        context: PatternContext,
    ) -> Vec<PatternElement> {
        let mut elements = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Node(child) if child.kind() == SyntaxKind::Pattern => {
                    elements.push(PatternElement::Pattern(self.lower_pattern(
                        body,
                        Some(child),
                        node.span(),
                        context,
                    )));
                }
                SyntaxElement::Node(child) if child.kind() == SyntaxKind::PatternList => {
                    elements.extend(self.lower_pattern_elements(body, &child, context));
                }
                SyntaxElement::Token(token) if token.kind() == SyntaxKind::Range => {
                    elements.push(PatternElement::Rest(self.written(token.span())));
                }
                SyntaxElement::Node(child) => {
                    elements.push(PatternElement::Pattern(self.lower_pattern(
                        body,
                        Some(child),
                        node.span(),
                        context,
                    )));
                }
                SyntaxElement::Token(_) => {}
            }
        }
        elements
    }

    fn lower_field_patterns(
        &mut self,
        body: crate::BodyId,
        node: &SyntaxNode,
        context: PatternContext,
    ) -> Vec<FieldPattern> {
        direct_nodes(node)
            .into_iter()
            .map(|field| {
                if let Some(rest) = direct_token(&field, SyntaxKind::Range) {
                    return FieldPattern::Rest(self.written(rest.span()));
                }
                let origin = self.written(field.span());
                let name = self.lower_name_token(first_name_token(&field), field.span());
                let pattern = direct_child(&field, SyntaxKind::Pattern)
                    .map(|pattern| self.lower_pattern(body, Some(pattern), field.span(), context));
                FieldPattern::Field {
                    name,
                    pattern,
                    origin,
                }
            })
            .collect()
    }

    fn invalid_pattern(&mut self, body: crate::BodyId, span: eglur_source::FileSpan) -> PatternId {
        let origin = self.record_invalid(InvalidCategory::Pattern, span);
        self.alloc_pattern(body, Pattern::new(origin, PatternKind::Invalid))
    }
}

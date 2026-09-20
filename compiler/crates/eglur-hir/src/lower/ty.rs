use eglur_syntax::{SyntaxKind, SyntaxNode};

use super::{Builder, direct_child, direct_children, direct_nodes, direct_token, is_type_kind};
use crate::{InvalidCategory, Mutability, PointerMutability, TypeRef, TypeRefId, TypeRefKind};

impl Builder {
    pub(super) fn lower_type(
        &mut self,
        node: Option<SyntaxNode>,
        fallback: eglur_source::FileSpan,
    ) -> TypeRefId {
        let Some(node) = node else {
            return self.invalid_type(fallback);
        };
        if node.kind() == SyntaxKind::TypeExpr {
            let nested = direct_nodes(&node)
                .into_iter()
                .find(|child| is_type_kind(child.kind()));
            if direct_token(&node, SyntaxKind::LeftParen).is_none() {
                return self.lower_type(nested, node.span());
            }
            let origin = self.written(node.span());
            let inner = self.lower_type(nested, node.span());
            return self.alloc_type(TypeRef::new(origin, TypeRefKind::Group(inner)));
        }
        let origin = self.written(node.span());
        let kind = match node.kind() {
            SyntaxKind::RefType => {
                let referent = self.lower_first_nested_type(&node);
                TypeRefKind::Reference {
                    mutability: if direct_token(&node, SyntaxKind::MutKw).is_some() {
                        Mutability::Mutable
                    } else {
                        Mutability::Immutable
                    },
                    referent,
                }
            }
            SyntaxKind::PointerType => {
                let referent = self.lower_first_nested_type(&node);
                let mutability = if direct_token(&node, SyntaxKind::MutKw).is_some() {
                    Some(PointerMutability::Mutable)
                } else if direct_token(&node, SyntaxKind::ConstKw).is_some() {
                    Some(PointerMutability::Const)
                } else {
                    None
                };
                if let Some(mutability) = mutability {
                    TypeRefKind::Pointer {
                        mutability,
                        referent,
                    }
                } else {
                    self.invalid
                        .push(crate::InvalidOccurrence::new(InvalidCategory::Type, origin));
                    TypeRefKind::Invalid
                }
            }
            SyntaxKind::PathType => {
                let path = self.lower_path(Some(node.clone()), node.span());
                let arguments = direct_child(&node, SyntaxKind::TypeArgumentList)
                    .map(|arguments| self.lower_type_list_container(&arguments))
                    .unwrap_or_default();
                TypeRefKind::Path {
                    path,
                    arguments: arguments.into_boxed_slice(),
                }
            }
            SyntaxKind::DynType => {
                let trait_ref =
                    self.lower_type(direct_child(&node, SyntaxKind::PathType), node.span());
                TypeRefKind::Dynamic { trait_ref }
            }
            SyntaxKind::TupleType => {
                TypeRefKind::Tuple(self.lower_tuple_types(&node).into_boxed_slice())
            }
            SyntaxKind::FnType => {
                let parameters = direct_child(&node, SyntaxKind::TypeList)
                    .map(|list| self.lower_types_in(&list))
                    .unwrap_or_default();
                let result = self.lower_type(
                    direct_nodes(&node)
                        .into_iter()
                        .rfind(|child| child.kind() == SyntaxKind::TypeExpr),
                    node.span(),
                );
                TypeRefKind::Function {
                    parameters: parameters.into_boxed_slice(),
                    result,
                }
            }
            SyntaxKind::ErrorNode => {
                self.invalid
                    .push(crate::InvalidOccurrence::new(InvalidCategory::Type, origin));
                TypeRefKind::Invalid
            }
            SyntaxKind::TypeExpr => unreachable!("type wrappers were handled before allocation"),
            _ => {
                self.invalid
                    .push(crate::InvalidOccurrence::new(InvalidCategory::Type, origin));
                TypeRefKind::Invalid
            }
        };
        self.alloc_type(TypeRef::new(origin, kind))
    }

    fn lower_first_nested_type(&mut self, node: &SyntaxNode) -> TypeRefId {
        self.lower_type(
            direct_nodes(node)
                .into_iter()
                .find(|child| is_type_kind(child.kind())),
            node.span(),
        )
    }

    pub(super) fn lower_type_list_container(&mut self, node: &SyntaxNode) -> Vec<TypeRefId> {
        direct_child(node, SyntaxKind::TypeList)
            .map(|list| self.lower_types_in(&list))
            .unwrap_or_else(|| self.lower_types_in(node))
    }

    fn lower_types_in(&mut self, node: &SyntaxNode) -> Vec<TypeRefId> {
        direct_nodes(node)
            .into_iter()
            .filter(|child| child.kind() == SyntaxKind::TypeExpr)
            .map(|child| self.lower_type(Some(child), node.span()))
            .collect()
    }

    fn lower_tuple_types(&mut self, node: &SyntaxNode) -> Vec<TypeRefId> {
        let mut types = direct_children(node, SyntaxKind::TypeExpr)
            .into_iter()
            .map(|child| self.lower_type(Some(child), node.span()))
            .collect::<Vec<_>>();
        if let Some(list) = direct_child(node, SyntaxKind::TypeList) {
            types.extend(self.lower_types_in(&list));
        }
        types
    }

    fn invalid_type(&mut self, span: eglur_source::FileSpan) -> TypeRefId {
        let origin = self.record_invalid(InvalidCategory::Type, span);
        self.alloc_type(TypeRef::new(origin, TypeRefKind::Invalid))
    }
}

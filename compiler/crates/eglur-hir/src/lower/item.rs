use eglur_syntax::ast::{AstNode, SourceElement, SourceFile};
use eglur_syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

use super::body::expression_children;
use super::{
    Builder, direct_child, direct_children, direct_nodes, direct_token, direct_tokens,
    first_name_token, is_trivia,
};
use crate::{
    AssociatedTypeBinding, AssociatedTypeDecl, Attribute, AttributeArgument, AttributeMeta,
    AttributeValue, BodyExpression, ConstDecl, EnumDecl, EnumVariant, EnumVariantField,
    EnumVariantPayload, ExternDecl, FunctionDecl, GenericParameter, ImplDecl, ImportDecl,
    ImportEntry, ImportSource, InvalidCategory, Item, ItemId, ItemKind, ModuleDecl, Mutability,
    Parameter, PropertyDecl, StorageFieldDecl, StorageFieldSyntax, StructDecl, TraitDecl,
    TypeAliasDecl, Visibility,
};

impl Builder {
    pub(super) fn lower_root(&mut self, root: SourceFile) {
        for element in root.elements() {
            match element {
                SourceElement::Module(module) => {
                    if self.module.is_none() {
                        self.module = Some(self.lower_module(module.syntax()));
                    } else {
                        let item = self.lower_invalid_item(module.syntax());
                        self.roots.push(item);
                    }
                }
                SourceElement::Item(item) => {
                    let item = self.lower_item(item.syntax().clone());
                    self.roots.push(item);
                }
                SourceElement::Invalid(invalid) => {
                    let item = self.lower_invalid_item(invalid.syntax());
                    self.roots.push(item);
                }
                SourceElement::Unexpected(unexpected) => {
                    let item = self.lower_invalid_item(unexpected.syntax());
                    self.roots.push(item);
                }
            }
        }
    }

    fn lower_module(&mut self, node: &SyntaxNode) -> ModuleDecl {
        ModuleDecl::new(
            self.lower_path(direct_child(node, SyntaxKind::Path), node.span()),
            self.written(node.span()),
        )
    }

    fn lower_item(&mut self, node: SyntaxNode) -> ItemId {
        let origin = self.written(node.span());
        let attributes = direct_child(&node, SyntaxKind::AttributeList)
            .map(|attributes| self.lower_attributes(&attributes))
            .unwrap_or_default();
        let visibility = if direct_token(&node, SyntaxKind::PubKw).is_some() {
            Visibility::Public
        } else {
            Visibility::Private
        };
        let kind = match node.kind() {
            SyntaxKind::ImportDecl => ItemKind::Import(self.lower_import(&node)),
            SyntaxKind::FnDecl => ItemKind::Function(self.lower_function(&node)),
            SyntaxKind::ExternDecl => ItemKind::Extern(self.lower_extern(&node)),
            SyntaxKind::TraitDecl => ItemKind::Trait(self.lower_trait(&node)),
            SyntaxKind::ImplDecl => ItemKind::Impl(self.lower_impl(&node)),
            SyntaxKind::StructDecl => ItemKind::Struct(self.lower_struct(&node)),
            SyntaxKind::EnumDecl => ItemKind::Enum(self.lower_enum(&node)),
            SyntaxKind::TypeDecl => ItemKind::TypeAlias(self.lower_type_alias(&node)),
            SyntaxKind::ConstDecl => ItemKind::Const(self.lower_const(&node)),
            SyntaxKind::AssocTypeDecl => ItemKind::AssociatedType(AssociatedTypeDecl {
                name: self.lower_name_token(first_name_token(&node), node.span()),
            }),
            SyntaxKind::AssocTypeBinding => {
                ItemKind::AssociatedTypeBinding(AssociatedTypeBinding {
                    name: self.lower_name_token(first_name_token(&node), node.span()),
                    value: self.lower_type(direct_child(&node, SyntaxKind::TypeExpr), node.span()),
                })
            }
            SyntaxKind::StorageFieldDecl => ItemKind::StorageField(self.lower_storage_field(&node)),
            SyntaxKind::PropertyDecl => ItemKind::Property(self.lower_property(&node)),
            SyntaxKind::ErrorNode => {
                self.invalid
                    .push(crate::InvalidOccurrence::new(InvalidCategory::Item, origin));
                ItemKind::Invalid
            }
            _ => {
                self.invalid
                    .push(crate::InvalidOccurrence::new(InvalidCategory::Item, origin));
                ItemKind::Invalid
            }
        };
        self.alloc_item(Item::new(
            origin,
            attributes.into_boxed_slice(),
            visibility,
            kind,
        ))
    }

    fn lower_import(&mut self, node: &SyntaxNode) -> ImportDecl {
        let path = self.lower_path(direct_child(node, SyntaxKind::Path), node.span());
        let source = if let Some(group) = direct_child(node, SyntaxKind::ImportGroup) {
            let entries = direct_children(&group, SyntaxKind::ImportEntry)
                .into_iter()
                .map(|entry| {
                    let origin = self.written(entry.span());
                    let mut names = direct_tokens(&entry).into_iter().filter(|token| {
                        matches!(
                            token.kind(),
                            SyntaxKind::Identifier | SyntaxKind::BangIdentifier
                        )
                    });
                    let name = self.lower_name_token(names.next(), entry.span());
                    let alias = names
                        .next()
                        .map(|token| self.lower_name_token(Some(token), entry.span()));
                    ImportEntry {
                        name,
                        alias,
                        origin,
                    }
                })
                .collect::<Vec<_>>();
            ImportSource::Group {
                prefix: path,
                entries: entries.into_boxed_slice(),
            }
        } else {
            let alias = direct_token(node, SyntaxKind::AsKw).and_then(|as_token| {
                direct_tokens(node)
                    .into_iter()
                    .skip_while(|token| token.span() != as_token.span())
                    .skip(1)
                    .find(|token| {
                        matches!(
                            token.kind(),
                            SyntaxKind::Identifier | SyntaxKind::BangIdentifier
                        )
                    })
                    .map(|token| self.lower_name_token(Some(token), node.span()))
            });
            ImportSource::Single { path, alias }
        };
        ImportDecl { source }
    }

    fn lower_function(&mut self, node: &SyntaxNode) -> FunctionDecl {
        let signature = direct_child(node, SyntaxKind::FnSignature);
        let body = self.new_body();
        let parameters = signature
            .as_ref()
            .and_then(|signature| direct_child(signature, SyntaxKind::ParameterList))
            .map(|parameters| self.lower_parameters(body, &parameters))
            .unwrap_or_default();
        let generic_parameters = signature
            .as_ref()
            .and_then(|signature| direct_child(signature, SyntaxKind::GenericParameterList))
            .map(|parameters| self.lower_generic_parameters(&parameters))
            .unwrap_or_default();
        let return_type = signature.as_ref().and_then(|signature| {
            direct_child(signature, SyntaxKind::TypeExpr)
                .map(|ty| self.lower_type(Some(ty), signature.span()))
        });
        let name = match signature.as_ref() {
            Some(signature) => self.lower_name_token(first_name_token(signature), signature.span()),
            None => self.invalid_name(node.span()),
        };
        FunctionDecl {
            name,
            overload: signature
                .as_ref()
                .is_some_and(|signature| direct_token(signature, SyntaxKind::OverloadKw).is_some()),
            generic_parameters: generic_parameters.into_boxed_slice(),
            parameters: parameters.into_boxed_slice(),
            return_type,
            body,
            body_form: self.lower_function_body(body, node),
        }
    }

    fn lower_extern(&mut self, node: &SyntaxNode) -> ExternDecl {
        let body = self.new_body();
        let abi = self.lower_literal_token(direct_token(node, SyntaxKind::String), node.span());
        let parameters = direct_child(node, SyntaxKind::ParameterList)
            .map(|parameters| self.lower_parameters(body, &parameters))
            .unwrap_or_default();
        let return_type = direct_child(node, SyntaxKind::TypeExpr)
            .map(|ty| self.lower_type(Some(ty), node.span()));
        ExternDecl {
            abi,
            name: self.lower_name_token(first_name_token(node), node.span()),
            parameters: parameters.into_boxed_slice(),
            return_type,
            body,
        }
    }

    fn lower_trait(&mut self, node: &SyntaxNode) -> TraitDecl {
        let name_token = direct_tokens(node)
            .into_iter()
            .rfind(|token| token.kind() == SyntaxKind::Identifier);
        TraitDecl {
            name: self.lower_name_token(name_token, node.span()),
            closed: direct_tokens(node)
                .iter()
                .any(|token| token.kind() == SyntaxKind::Identifier && token.text() == "closed"),
            generic_parameters: direct_child(node, SyntaxKind::GenericParameterList)
                .map(|parameters| self.lower_generic_parameters(&parameters))
                .unwrap_or_default()
                .into_boxed_slice(),
            items: direct_child(node, SyntaxKind::TraitItemList)
                .map(|items| self.lower_nested_items(&items))
                .unwrap_or_default()
                .into_boxed_slice(),
        }
    }

    fn lower_impl(&mut self, node: &SyntaxNode) -> ImplDecl {
        let mut types = direct_children(node, SyntaxKind::TypeExpr).into_iter();
        ImplDecl {
            generic_parameters: direct_child(node, SyntaxKind::GenericParameterList)
                .map(|parameters| self.lower_generic_parameters(&parameters))
                .unwrap_or_default()
                .into_boxed_slice(),
            negative: direct_token(node, SyntaxKind::Bang).is_some(),
            trait_ref: self.lower_type(types.next(), node.span()),
            target: self.lower_type(types.next(), node.span()),
            items: direct_child(node, SyntaxKind::ImplItemList)
                .map(|items| self.lower_nested_items(&items))
                .unwrap_or_default()
                .into_boxed_slice(),
        }
    }

    fn lower_struct(&mut self, node: &SyntaxNode) -> StructDecl {
        StructDecl {
            name: self.lower_name_token(first_name_token(node), node.span()),
            generic_parameters: direct_child(node, SyntaxKind::GenericParameterList)
                .map(|parameters| self.lower_generic_parameters(&parameters))
                .unwrap_or_default()
                .into_boxed_slice(),
            base: direct_child(node, SyntaxKind::TypeExpr)
                .map(|ty| self.lower_type(Some(ty), node.span())),
            items: direct_child(node, SyntaxKind::StructItemList)
                .map(|items| self.lower_nested_items(&items))
                .unwrap_or_default()
                .into_boxed_slice(),
        }
    }

    fn lower_enum(&mut self, node: &SyntaxNode) -> EnumDecl {
        EnumDecl {
            name: self.lower_name_token(first_name_token(node), node.span()),
            generic_parameters: direct_child(node, SyntaxKind::GenericParameterList)
                .map(|parameters| self.lower_generic_parameters(&parameters))
                .unwrap_or_default()
                .into_boxed_slice(),
            variants: direct_children(node, SyntaxKind::EnumVariant)
                .into_iter()
                .map(|variant| self.lower_enum_variant(&variant))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    fn lower_enum_variant(&mut self, node: &SyntaxNode) -> EnumVariant {
        let payload = if let Some(types) = direct_child(node, SyntaxKind::TypeList) {
            EnumVariantPayload::Tuple(
                direct_children(&types, SyntaxKind::TypeExpr)
                    .into_iter()
                    .map(|ty| self.lower_type(Some(ty), types.span()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )
        } else if let Some(fields) = direct_child(node, SyntaxKind::EnumVariantFieldList) {
            EnumVariantPayload::Named(
                direct_children(&fields, SyntaxKind::EnumVariantField)
                    .into_iter()
                    .map(|field| EnumVariantField {
                        origin: self.written(field.span()),
                        name: self.lower_name_token(first_name_token(&field), field.span()),
                        ty: self
                            .lower_type(direct_child(&field, SyntaxKind::TypeExpr), field.span()),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )
        } else {
            EnumVariantPayload::Absent
        };
        EnumVariant {
            name: self.lower_name_token(first_name_token(node), node.span()),
            attributes: direct_child(node, SyntaxKind::AttributeList)
                .map(|attributes| self.lower_attributes(&attributes))
                .unwrap_or_default()
                .into_boxed_slice(),
            visibility: if direct_token(node, SyntaxKind::PubKw).is_some() {
                Visibility::Public
            } else {
                Visibility::Private
            },
            payload,
            origin: self.written(node.span()),
        }
    }

    fn lower_type_alias(&mut self, node: &SyntaxNode) -> TypeAliasDecl {
        TypeAliasDecl {
            name: self.lower_name_token(first_name_token(node), node.span()),
            generic_parameters: direct_child(node, SyntaxKind::GenericParameterList)
                .map(|parameters| self.lower_generic_parameters(&parameters))
                .unwrap_or_default()
                .into_boxed_slice(),
            value: self.lower_type(direct_child(node, SyntaxKind::TypeExpr), node.span()),
        }
    }

    fn lower_const(&mut self, node: &SyntaxNode) -> ConstDecl {
        let body = self.new_body();
        let value = self.lower_expr(
            body,
            expression_children(node).into_iter().next(),
            node.span(),
        );
        ConstDecl {
            name: self.lower_name_token(first_name_token(node), node.span()),
            ty: self.lower_type(direct_child(node, SyntaxKind::TypeExpr), node.span()),
            body,
            value,
        }
    }

    fn lower_storage_field(&mut self, node: &SyntaxNode) -> StorageFieldDecl {
        let initializer = expression_children(node)
            .into_iter()
            .next()
            .map(|expression| {
                let body = self.new_body();
                BodyExpression {
                    body,
                    expression: self.lower_expr(body, Some(expression), node.span()),
                }
            });
        StorageFieldDecl {
            syntax: if direct_token(node, SyntaxKind::VarKw).is_some() {
                StorageFieldSyntax::Var
            } else {
                StorageFieldSyntax::Field
            },
            mutability: if direct_token(node, SyntaxKind::VarKw).is_some()
                || direct_token(node, SyntaxKind::MutKw).is_some()
            {
                Mutability::Mutable
            } else {
                Mutability::Immutable
            },
            name: self.lower_name_token(first_name_token(node), node.span()),
            ty: self.lower_type(direct_child(node, SyntaxKind::TypeExpr), node.span()),
            initializer,
        }
    }

    fn lower_property(&mut self, node: &SyntaxNode) -> PropertyDecl {
        PropertyDecl {
            mutability: if direct_token(node, SyntaxKind::MutKw).is_some() {
                Mutability::Mutable
            } else {
                Mutability::Immutable
            },
            name: self.lower_name_token(first_name_token(node), node.span()),
            ty: self.lower_type(direct_child(node, SyntaxKind::TypeExpr), node.span()),
            accessors: direct_child(node, SyntaxKind::PropertyAccessorList)
                .map(|items| self.lower_nested_items(&items))
                .unwrap_or_default()
                .into_boxed_slice(),
        }
    }

    fn lower_nested_items(&mut self, node: &SyntaxNode) -> Vec<ItemId> {
        direct_nodes(node)
            .into_iter()
            .map(|child| {
                if is_item_kind(child.kind()) {
                    self.lower_item(child)
                } else {
                    self.lower_invalid_item(&child)
                }
            })
            .collect()
    }

    fn lower_parameters(&mut self, body: crate::BodyId, node: &SyntaxNode) -> Vec<Parameter> {
        direct_children(node, SyntaxKind::Parameter)
            .into_iter()
            .map(|parameter| Parameter {
                origin: self.written(parameter.span()),
                pattern: self.lower_pattern(
                    body,
                    direct_child(&parameter, SyntaxKind::Pattern),
                    parameter.span(),
                ),
                ty: direct_child(&parameter, SyntaxKind::TypeExpr)
                    .map(|ty| self.lower_type(Some(ty), parameter.span())),
            })
            .collect()
    }

    fn lower_generic_parameters(&mut self, node: &SyntaxNode) -> Vec<GenericParameter> {
        direct_children(node, SyntaxKind::GenericParameter)
            .into_iter()
            .map(|parameter| GenericParameter {
                origin: self.written(parameter.span()),
                name: self.lower_name_token(first_name_token(&parameter), parameter.span()),
                bounds: direct_child(&parameter, SyntaxKind::TraitBoundList)
                    .map(|bounds| {
                        direct_children(&bounds, SyntaxKind::TypeExpr)
                            .into_iter()
                            .map(|ty| self.lower_type(Some(ty), bounds.span()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
                    .into_boxed_slice(),
            })
            .collect()
    }

    fn lower_attributes(&mut self, node: &SyntaxNode) -> Vec<Attribute> {
        direct_children(node, SyntaxKind::Attribute)
            .into_iter()
            .map(|attribute| {
                let origin = self.written(attribute.span());
                let meta = direct_child(&attribute, SyntaxKind::AttributeMeta)
                    .map(|meta| self.lower_attribute_meta(&meta))
                    .unwrap_or_else(|| self.invalid_attribute_meta(attribute.span()));
                Attribute { meta, origin }
            })
            .collect()
    }

    fn lower_attribute_meta(&mut self, node: &SyntaxNode) -> AttributeMeta {
        let origin = self.written(node.span());
        let path = self.lower_path(direct_child(node, SyntaxKind::Path), node.span());
        let arguments = if direct_token(node, SyntaxKind::LeftParen).is_some() {
            split_attribute_arguments(node)
                .into_iter()
                .filter_map(|elements| self.lower_attribute_argument(elements, node.span()))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        AttributeMeta {
            path,
            arguments: arguments.into_boxed_slice(),
            origin,
        }
    }

    fn lower_attribute_argument(
        &mut self,
        elements: Vec<SyntaxElement>,
        fallback: eglur_source::FileSpan,
    ) -> Option<AttributeArgument> {
        let argument_span = syntax_elements_span(&elements).unwrap_or(fallback);
        let mut tokens = elements.iter().filter_map(|element| match element {
            SyntaxElement::Token(token) if !is_trivia(token.kind()) => Some(token.clone()),
            SyntaxElement::Node(_) | SyntaxElement::Token(_) => None,
        });
        let first = tokens.next();
        let named = first
            .as_ref()
            .filter(|token| token.kind() == SyntaxKind::Identifier)
            .filter(|_| {
                tokens
                    .clone()
                    .any(|token| token.kind() == SyntaxKind::Equal)
            });
        let name = named.map(|token| self.lower_name_token(Some(token.clone()), argument_span));
        let value = elements.iter().find_map(|element| match element {
            SyntaxElement::Node(node) if node.kind() == SyntaxKind::AttributeMeta => Some(
                AttributeValue::Meta(Box::new(self.lower_attribute_meta(node))),
            ),
            SyntaxElement::Token(token)
                if matches!(
                    token.kind(),
                    SyntaxKind::Integer
                        | SyntaxKind::Float
                        | SyntaxKind::String
                        | SyntaxKind::TrueKw
                        | SyntaxKind::FalseKw
                ) =>
            {
                Some(AttributeValue::Literal(
                    self.lower_literal_token(Some(token.clone()), argument_span),
                ))
            }
            SyntaxElement::Node(_) | SyntaxElement::Token(_) => None,
        });
        if elements.is_empty() {
            return None;
        }
        let origin = self.written(argument_span);
        Some(AttributeArgument {
            name,
            value: value.unwrap_or_else(|| {
                let invalid = self.record_invalid(InvalidCategory::Attribute, argument_span);
                AttributeValue::Invalid(invalid)
            }),
            origin,
        })
    }

    fn invalid_attribute_meta(&mut self, span: eglur_source::FileSpan) -> AttributeMeta {
        let origin = self.record_invalid(InvalidCategory::Attribute, span);
        AttributeMeta {
            path: self.lower_path(None, span),
            arguments: Box::new([]),
            origin,
        }
    }

    fn lower_invalid_item(&mut self, node: &SyntaxNode) -> ItemId {
        let origin = self.record_invalid(InvalidCategory::Item, node.span());
        self.alloc_item(Item::new(
            origin,
            Box::new([]),
            Visibility::Private,
            ItemKind::Invalid,
        ))
    }
}

fn is_item_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ImportDecl
            | SyntaxKind::FnDecl
            | SyntaxKind::ExternDecl
            | SyntaxKind::TraitDecl
            | SyntaxKind::ImplDecl
            | SyntaxKind::StructDecl
            | SyntaxKind::EnumDecl
            | SyntaxKind::TypeDecl
            | SyntaxKind::ConstDecl
            | SyntaxKind::AssocTypeDecl
            | SyntaxKind::AssocTypeBinding
            | SyntaxKind::StorageFieldDecl
            | SyntaxKind::PropertyDecl
    )
}

fn split_attribute_arguments(node: &SyntaxNode) -> Vec<Vec<SyntaxElement>> {
    let mut arguments = Vec::new();
    let mut current = Vec::new();
    let mut inside = false;
    for element in node.children_with_tokens() {
        match &element {
            SyntaxElement::Node(child) if child.kind() == SyntaxKind::Path && !inside => {}
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::LeftParen => {
                inside = true;
            }
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::RightParen => {
                if !current.is_empty() {
                    arguments.push(std::mem::take(&mut current));
                }
                break;
            }
            SyntaxElement::Token(token) if inside && token.kind() == SyntaxKind::Comma => {
                arguments.push(std::mem::take(&mut current));
            }
            _ if inside => current.push(element),
            _ => {}
        }
    }
    arguments
        .into_iter()
        .filter(|elements| {
            elements.iter().any(|element| match element {
                SyntaxElement::Node(_) => true,
                SyntaxElement::Token(token) => !is_trivia(token.kind()),
            })
        })
        .collect()
}

fn syntax_elements_span(elements: &[SyntaxElement]) -> Option<eglur_source::FileSpan> {
    let first = elements.iter().find_map(semantic_element_span)?;
    let last = elements.iter().rev().find_map(semantic_element_span)?;
    if first.file() != last.file() {
        return None;
    }
    let range = eglur_source::ByteRange::new(first.range().start(), last.range().end())?;
    Some(eglur_source::FileSpan::new(first.file(), range))
}

fn semantic_element_span(element: &SyntaxElement) -> Option<eglur_source::FileSpan> {
    match element {
        SyntaxElement::Node(node) => Some(node.span()),
        SyntaxElement::Token(token) if !is_trivia(token.kind()) => Some(token.span()),
        SyntaxElement::Token(_) => None,
    }
}

use crate::event::{CompletedMarker, Marker};
use crate::{SyntaxDiagnosticKind, SyntaxKind, TokenKind};

use super::Parser;

#[derive(Clone, Copy)]
enum FunctionBody {
    Required,
    Optional,
}

impl Parser<'_> {
    pub(super) fn parse_compilation_unit(&mut self) {
        self.skip_newlines();
        if self.at(TokenKind::ModuleKw) {
            let region_start = self.logical_position();
            let declaration = self.parse_module_declaration();
            if !self.finish_top_level_region(region_start, declaration) {
                return;
            }
        }

        while !self.at_eof() && !self.terminal() {
            let region_start = self.logical_position();
            let declaration = self.parse_top_level_item();
            if !self.finish_top_level_region(region_start, declaration) {
                return;
            }
        }
        if !self.halted() && !self.terminal() {
            self.require_eof();
        }
    }

    fn finish_top_level_region(
        &mut self,
        region_start: usize,
        declaration: Option<CompletedMarker>,
    ) -> bool {
        if self.terminal() {
            return false;
        }
        if self.halted() {
            if let Some(declaration) = declaration {
                self.change_kind(declaration, SyntaxKind::ErrorNode);
            }
            return self.recover_top_level(region_start);
        }
        self.skip_newlines();
        !self.terminal()
    }

    fn parse_module_declaration(&mut self) -> Option<CompletedMarker> {
        let declaration = self.start();
        self.bump();
        self.parse_path(SyntaxKind::Path);
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::ModuleDecl))
    }

    fn parse_top_level_item(&mut self) -> Option<CompletedMarker> {
        if self.at(TokenKind::HashLeftBracket) {
            let declaration = self.start();
            self.parse_attribute_list();
            return self.parse_attributed_top_level_item(declaration);
        }

        let declaration = self.start();
        let public = self.eat(TokenKind::PubKw);
        match self.current_kind() {
            Some(TokenKind::ImportKw) => self.parse_import_declaration(declaration),
            Some(TokenKind::OverloadKw | TokenKind::FnKw) => {
                self.parse_function_declaration(declaration, FunctionBody::Required)
            }
            Some(TokenKind::ExternKw) => self.parse_extern_declaration(declaration),
            Some(TokenKind::TraitKw) => self.parse_trait_declaration(declaration),
            Some(TokenKind::StructKw) => self.parse_struct_declaration(declaration),
            Some(TokenKind::EnumKw) => self.parse_enum_declaration(declaration),
            Some(TokenKind::TypeKw) => self.parse_type_declaration(declaration),
            Some(TokenKind::ConstKw) => self.parse_const_declaration(declaration),
            Some(TokenKind::ImplKw) if !public => self.parse_impl_declaration(declaration),
            Some(TokenKind::Identifier) if self.at_contextual("closed") => {
                self.parse_trait_declaration(declaration)
            }
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                Some(self.complete(declaration, SyntaxKind::ErrorNode))
            }
        }
    }

    fn parse_attributed_top_level_item(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        let public = self.eat(TokenKind::PubKw);
        match self.current_kind() {
            Some(TokenKind::OverloadKw | TokenKind::FnKw) => {
                self.parse_function_declaration(declaration, FunctionBody::Required)
            }
            Some(TokenKind::StructKw) => self.parse_struct_declaration(declaration),
            Some(TokenKind::EnumKw) => self.parse_enum_declaration(declaration),
            Some(TokenKind::ImplKw) if !public => self.parse_impl_declaration(declaration),
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                Some(self.complete(declaration, SyntaxKind::ErrorNode))
            }
        }
    }

    fn parse_import_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.expect(TokenKind::ImportKw);
        if !self.at(TokenKind::Identifier) {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            return Some(self.complete(declaration, SyntaxKind::ImportDecl));
        }

        let path = self.start();
        self.bump();
        while self.at(TokenKind::ColonColon)
            && matches!(
                self.nth_kind(1),
                Some(TokenKind::Identifier | TokenKind::BangIdentifier)
            )
        {
            self.bump();
            self.bump();
        }
        self.complete(path, SyntaxKind::Path);

        if self.at(TokenKind::ColonColon) && self.nth_at(1, TokenKind::LeftBrace) {
            self.bump();
            self.parse_import_group();
        } else {
            self.parse_import_alias();
        }
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::ImportDecl))
    }

    fn parse_import_group(&mut self) {
        let group = self.start();
        self.expect(TokenKind::LeftBrace);
        self.skip_newlines();
        if !self.at(TokenKind::RightBrace) {
            loop {
                let entry = self.start();
                if matches!(
                    self.current_kind(),
                    Some(TokenKind::Identifier | TokenKind::BangIdentifier)
                ) {
                    self.bump();
                } else {
                    self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                }
                self.parse_import_alias();
                self.complete(entry, SyntaxKind::ImportEntry);

                self.skip_newlines();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.at(TokenKind::RightBrace) {
                    break;
                }
            }
        }
        self.expect_closing(TokenKind::RightBrace);
        self.complete(group, SyntaxKind::ImportGroup);
    }

    fn parse_import_alias(&mut self) {
        if self.eat(TokenKind::AsKw) {
            if matches!(
                self.current_kind(),
                Some(TokenKind::Identifier | TokenKind::BangIdentifier)
            ) {
                self.bump();
            } else {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            }
        }
    }

    fn parse_function_declaration(
        &mut self,
        declaration: Marker,
        body: FunctionBody,
    ) -> Option<CompletedMarker> {
        let signature = self.start();
        self.eat(TokenKind::OverloadKw);
        self.expect(TokenKind::FnKw);
        self.parse_function_name();
        if self.at(TokenKind::LeftBracket) {
            self.parse_generic_parameter_list();
        }
        self.parse_parameter_list();
        if self.eat(TokenKind::Arrow) {
            self.parse_type();
        }
        self.complete(signature, SyntaxKind::FnSignature);

        if self.at(TokenKind::LeftBrace) {
            self.parse_block_expression();
        } else if self.eat(TokenKind::Equal) {
            self.skip_newlines();
            self.parse_expression(false, true);
            self.line_end();
        } else if matches!(body, FunctionBody::Optional) {
            self.line_end();
        } else {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }
        Some(self.complete(declaration, SyntaxKind::FnDecl))
    }

    fn parse_function_name(&mut self) {
        if matches!(
            self.current_kind(),
            Some(TokenKind::Identifier | TokenKind::BangIdentifier)
        ) {
            self.bump();
        } else {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }
    }

    fn parse_parameter_list(&mut self) {
        let parameters = self.start();
        self.expect(TokenKind::LeftParen);
        self.skip_newlines();
        if !self.at(TokenKind::RightParen) {
            loop {
                let parameter = self.start();
                self.parse_pattern();
                self.expect(TokenKind::Colon);
                self.parse_type();
                self.complete(parameter, SyntaxKind::Parameter);

                self.skip_newlines();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.at(TokenKind::RightParen) {
                    break;
                }
            }
        }
        self.expect_closing(TokenKind::RightParen);
        self.complete(parameters, SyntaxKind::ParameterList);
    }

    fn parse_extern_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.bump();
        self.expect(TokenKind::String);
        self.expect(TokenKind::FnKw);
        self.parse_function_name();
        self.parse_parameter_list();
        if self.eat(TokenKind::Arrow) {
            self.parse_type();
        }
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::ExternDecl))
    }

    fn parse_generic_parameter_list(&mut self) {
        let parameters = self.start();
        self.expect(TokenKind::LeftBracket);
        self.skip_newlines();
        if self.at(TokenKind::RightBracket) {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        } else {
            loop {
                let parameter = self.start();
                self.expect_identifier();
                if self.eat(TokenKind::Colon) {
                    let bounds = self.start();
                    self.parse_type();
                    while self.eat(TokenKind::Plus) {
                        self.parse_type();
                    }
                    self.complete(bounds, SyntaxKind::TraitBoundList);
                }
                self.complete(parameter, SyntaxKind::GenericParameter);

                self.skip_newlines();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.at(TokenKind::RightBracket) {
                    break;
                }
            }
        }
        self.expect_closing(TokenKind::RightBracket);
        self.complete(parameters, SyntaxKind::GenericParameterList);
    }

    fn parse_trait_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.eat_contextual("closed");
        self.expect(TokenKind::TraitKw);
        self.expect_identifier();
        if self.at(TokenKind::LeftBracket) {
            self.parse_generic_parameter_list();
        }
        self.expect(TokenKind::LeftBrace);

        let items = self.start();
        self.skip_newlines();
        while !self.at(TokenKind::RightBrace) && !self.at_eof() && !self.halted() {
            self.parse_trait_item();
            self.skip_newlines();
        }
        self.complete(items, SyntaxKind::TraitItemList);
        self.expect_closing(TokenKind::RightBrace);
        Some(self.complete(declaration, SyntaxKind::TraitDecl))
    }

    fn parse_trait_item(&mut self) -> Option<CompletedMarker> {
        if self.at(TokenKind::HashLeftBracket) {
            let declaration = self.start();
            self.parse_attribute_list();
            if self.at(TokenKind::OverloadKw) || self.at(TokenKind::FnKw) {
                return self.parse_function_declaration(declaration, FunctionBody::Optional);
            }
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            return Some(self.complete(declaration, SyntaxKind::ErrorNode));
        }

        let declaration = self.start();
        match self.current_kind() {
            Some(TokenKind::OverloadKw | TokenKind::FnKw) => {
                self.parse_function_declaration(declaration, FunctionBody::Optional)
            }
            Some(TokenKind::TypeKw) => self.parse_associated_type_declaration(declaration),
            Some(TokenKind::FieldKw | TokenKind::VarKw) => {
                self.parse_storage_field_declaration(declaration)
            }
            Some(TokenKind::PropKw) => self.parse_property_declaration(declaration),
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                Some(self.complete(declaration, SyntaxKind::ErrorNode))
            }
        }
    }

    fn parse_associated_type_declaration(
        &mut self,
        declaration: Marker,
    ) -> Option<CompletedMarker> {
        self.bump();
        self.expect_identifier();
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::AssocTypeDecl))
    }

    fn parse_impl_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.expect(TokenKind::ImplKw);
        if self.at(TokenKind::LeftBracket) {
            self.parse_generic_parameter_list();
        }
        self.eat(TokenKind::Bang);
        self.parse_type();
        self.expect(TokenKind::ForKw);
        self.parse_type();
        self.expect(TokenKind::LeftBrace);

        let items = self.start();
        self.skip_newlines();
        while !self.at(TokenKind::RightBrace) && !self.at_eof() && !self.halted() {
            self.parse_impl_item();
            self.skip_newlines();
        }
        self.complete(items, SyntaxKind::ImplItemList);
        self.expect_closing(TokenKind::RightBrace);
        Some(self.complete(declaration, SyntaxKind::ImplDecl))
    }

    fn parse_impl_item(&mut self) -> Option<CompletedMarker> {
        if self.at(TokenKind::HashLeftBracket) {
            let declaration = self.start();
            self.parse_attribute_list();
            if self.at(TokenKind::OverloadKw) || self.at(TokenKind::FnKw) {
                return self.parse_function_declaration(declaration, FunctionBody::Required);
            }
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            return Some(self.complete(declaration, SyntaxKind::ErrorNode));
        }

        let declaration = self.start();
        match self.current_kind() {
            Some(TokenKind::OverloadKw | TokenKind::FnKw) => {
                self.parse_function_declaration(declaration, FunctionBody::Required)
            }
            Some(TokenKind::TypeKw) => self.parse_associated_type_binding(declaration),
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                Some(self.complete(declaration, SyntaxKind::ErrorNode))
            }
        }
    }

    fn parse_associated_type_binding(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.bump();
        self.expect_identifier();
        self.expect(TokenKind::Equal);
        self.parse_type();
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::AssocTypeBinding))
    }

    fn parse_struct_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.expect(TokenKind::StructKw);
        self.expect_identifier();
        if self.at(TokenKind::LeftBracket) {
            self.parse_generic_parameter_list();
        }
        if self.eat(TokenKind::Colon) {
            self.parse_type();
        }
        self.expect(TokenKind::LeftBrace);

        let items = self.start();
        self.skip_newlines();
        while !self.at(TokenKind::RightBrace) && !self.at_eof() && !self.halted() {
            self.parse_struct_item();
            self.skip_newlines();
        }
        self.complete(items, SyntaxKind::StructItemList);
        self.expect_closing(TokenKind::RightBrace);
        Some(self.complete(declaration, SyntaxKind::StructDecl))
    }

    fn parse_struct_item(&mut self) -> Option<CompletedMarker> {
        if self.at(TokenKind::HashLeftBracket) {
            let declaration = self.start();
            self.parse_attribute_list();
            self.eat(TokenKind::PubKw);
            if self.at(TokenKind::OverloadKw) || self.at(TokenKind::FnKw) {
                return self.parse_function_declaration(declaration, FunctionBody::Required);
            }
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            return Some(self.complete(declaration, SyntaxKind::ErrorNode));
        }

        let declaration = self.start();
        self.eat(TokenKind::PubKw);
        match self.current_kind() {
            Some(TokenKind::FieldKw | TokenKind::VarKw) => {
                self.parse_storage_field_declaration(declaration)
            }
            Some(TokenKind::PropKw) => self.parse_property_declaration(declaration),
            Some(TokenKind::OverloadKw | TokenKind::FnKw) => {
                self.parse_function_declaration(declaration, FunctionBody::Required)
            }
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                Some(self.complete(declaration, SyntaxKind::ErrorNode))
            }
        }
    }

    fn parse_storage_field_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        if self.eat(TokenKind::FieldKw) {
            self.eat(TokenKind::MutKw);
        } else {
            self.expect(TokenKind::VarKw);
        }
        self.expect_identifier();
        self.expect(TokenKind::Colon);
        self.parse_type();
        if self.eat(TokenKind::Equal) {
            self.parse_expression(false, true);
        }
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::StorageFieldDecl))
    }

    fn parse_property_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.bump();
        self.eat(TokenKind::MutKw);
        self.expect_identifier();
        self.expect(TokenKind::Colon);
        self.parse_type();
        self.expect(TokenKind::LeftBrace);

        let accessors = self.start();
        self.skip_newlines();
        while !self.at(TokenKind::RightBrace) && !self.at_eof() && !self.halted() {
            let accessor = self.start();
            if self.at(TokenKind::OverloadKw) || self.at(TokenKind::FnKw) {
                self.parse_function_declaration(accessor, FunctionBody::Required);
            } else {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                self.complete(accessor, SyntaxKind::ErrorNode);
            }
            self.skip_newlines();
        }
        self.complete(accessors, SyntaxKind::PropertyAccessorList);
        self.expect_closing(TokenKind::RightBrace);
        Some(self.complete(declaration, SyntaxKind::PropertyDecl))
    }

    fn parse_enum_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.expect(TokenKind::EnumKw);
        self.expect_identifier();
        if self.at(TokenKind::LeftBracket) {
            self.parse_generic_parameter_list();
        }
        self.expect(TokenKind::LeftBrace);
        self.skip_newlines();
        while !self.at(TokenKind::RightBrace) && !self.at_eof() && !self.halted() {
            self.parse_enum_variant();
            self.skip_newlines();
        }
        self.expect_closing(TokenKind::RightBrace);
        Some(self.complete(declaration, SyntaxKind::EnumDecl))
    }

    fn parse_enum_variant(&mut self) -> Option<CompletedMarker> {
        let variant = self.start();
        if self.at(TokenKind::HashLeftBracket) {
            self.parse_attribute_list();
        }
        self.eat(TokenKind::PubKw);
        self.expect_identifier();
        match self.current_kind() {
            Some(TokenKind::LeftParen) => self.parse_enum_tuple_payload(),
            Some(TokenKind::LeftBrace) => self.parse_enum_named_payload(),
            _ => {}
        }
        self.line_end();
        Some(self.complete(variant, SyntaxKind::EnumVariant))
    }

    fn parse_enum_tuple_payload(&mut self) {
        let types = self.start();
        self.bump();
        self.skip_newlines();
        if !self.at(TokenKind::RightParen) {
            loop {
                self.parse_type();
                self.skip_newlines();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.at(TokenKind::RightParen) {
                    break;
                }
            }
        }
        self.expect_closing(TokenKind::RightParen);
        self.complete(types, SyntaxKind::TypeList);
    }

    fn parse_enum_named_payload(&mut self) {
        let fields = self.start();
        self.bump();
        self.skip_newlines();
        if self.at(TokenKind::RightBrace) {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        } else {
            loop {
                let field = self.start();
                self.expect_identifier();
                self.expect(TokenKind::Colon);
                self.parse_type();
                self.complete(field, SyntaxKind::EnumVariantField);
                self.skip_newlines();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.at(TokenKind::RightBrace) {
                    break;
                }
            }
        }
        self.expect_closing(TokenKind::RightBrace);
        self.complete(fields, SyntaxKind::EnumVariantFieldList);
    }

    fn parse_type_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.bump();
        self.expect_identifier();
        if self.at(TokenKind::LeftBracket) {
            self.parse_generic_parameter_list();
        }
        self.expect(TokenKind::Equal);
        self.parse_type();
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::TypeDecl))
    }

    fn parse_const_declaration(&mut self, declaration: Marker) -> Option<CompletedMarker> {
        self.bump();
        self.expect_identifier();
        self.expect(TokenKind::Colon);
        self.parse_type();
        self.expect(TokenKind::Equal);
        self.parse_expression(false, true);
        self.line_end();
        Some(self.complete(declaration, SyntaxKind::ConstDecl))
    }

    fn parse_attribute_list(&mut self) -> Option<CompletedMarker> {
        let attributes = self.start();
        while self.at(TokenKind::HashLeftBracket) && !self.halted() {
            self.parse_attribute();
            self.skip_newlines();
        }
        Some(self.complete(attributes, SyntaxKind::AttributeList))
    }

    fn parse_attribute(&mut self) -> Option<CompletedMarker> {
        let attribute = self.start();
        self.bump();
        self.parse_attribute_meta();
        self.expect_closing(TokenKind::RightBracket);
        Some(self.complete(attribute, SyntaxKind::Attribute))
    }

    fn parse_attribute_meta(&mut self) -> Option<CompletedMarker> {
        if !self.enter_recursive() {
            return None;
        }
        let meta = self.start();
        self.parse_path(SyntaxKind::Path);
        if self.eat(TokenKind::LeftParen) {
            self.skip_newlines();
            if !self.at(TokenKind::RightParen) {
                loop {
                    self.parse_attribute_argument();
                    self.skip_newlines();
                    if !self.eat(TokenKind::Comma) {
                        break;
                    }
                    self.skip_newlines();
                    if self.at(TokenKind::RightParen) {
                        break;
                    }
                }
            }
            self.expect_closing(TokenKind::RightParen);
        }
        let completed = self.complete(meta, SyntaxKind::AttributeMeta);
        self.leave_recursive();
        Some(completed)
    }

    fn parse_attribute_argument(&mut self) {
        if self.at(TokenKind::Identifier) && self.nth_at(1, TokenKind::Equal) {
            self.bump();
            self.bump();
            self.parse_attribute_value();
        } else {
            self.parse_attribute_value();
        }
    }

    fn parse_attribute_value(&mut self) {
        match self.current_kind() {
            Some(
                TokenKind::Integer
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::TrueKw
                | TokenKind::FalseKw,
            ) => self.bump(),
            Some(TokenKind::Identifier) => {
                self.parse_attribute_meta();
            }
            _ => self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken),
        }
    }
}

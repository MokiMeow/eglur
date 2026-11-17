use crate::event::CompletedMarker;
use crate::{SyntaxDiagnosticKind, SyntaxKind, TokenKind};

use super::Parser;

impl Parser<'_> {
    pub(super) fn parse_type(&mut self) -> Option<CompletedMarker> {
        if !self.enter_recursive() {
            return None;
        }
        let type_expression = self.start();
        let parsed = self.parse_reference_type();
        if parsed.is_none() && !self.halted() {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }
        let completed = self.complete(type_expression, SyntaxKind::TypeExpr);
        self.leave_recursive();
        Some(completed)
    }

    pub(super) fn parse_trait_ref(&mut self) -> Option<CompletedMarker> {
        if !self.at(TokenKind::Identifier) || self.at_function_type() {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }

        // Consume an invalid type for recovery without admitting it as a trait bound.
        self.parse_type()
    }

    pub(super) fn parse_type_argument_list(&mut self) -> Option<CompletedMarker> {
        let arguments = self.start();
        let opening = self.logical_position();
        self.expect(TokenKind::LeftBracket);
        self.skip_newlines();
        if self.at(TokenKind::RightBracket) {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        } else {
            self.parse_type_list(TokenKind::RightBracket);
        }
        if !self.at(TokenKind::RightBracket)
            && !self.halted()
            && self.matching_delimiter(opening).is_some()
        {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }
        self.expect_closing(TokenKind::RightBracket);
        Some(self.complete(arguments, SyntaxKind::TypeArgumentList))
    }

    fn parse_reference_type(&mut self) -> Option<CompletedMarker> {
        match self.current_kind() {
            Some(TokenKind::Ampersand) => {
                let reference = self.start();
                self.bump();
                self.eat(TokenKind::MutKw);
                if self.enter_recursive() {
                    self.parse_reference_type();
                    self.leave_recursive();
                }
                Some(self.complete(reference, SyntaxKind::RefType))
            }
            Some(TokenKind::Star) => {
                let pointer = self.start();
                self.bump();
                if !self.eat(TokenKind::ConstKw) && !self.eat(TokenKind::MutKw) {
                    self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                }
                if self.enter_recursive() {
                    self.parse_reference_type();
                    self.leave_recursive();
                }
                Some(self.complete(pointer, SyntaxKind::PointerType))
            }
            _ => self.parse_primary_type(),
        }
    }

    fn parse_primary_type(&mut self) -> Option<CompletedMarker> {
        match self.current_kind() {
            Some(TokenKind::DynKw) => self.parse_dynamic_type(),
            Some(TokenKind::LeftParen) => self.parse_parenthesized_type(),
            Some(TokenKind::Identifier) if self.at_function_type() => self.parse_function_type(),
            Some(TokenKind::Identifier) => self.parse_path_type(),
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                None
            }
        }
    }

    fn at_function_type(&self) -> bool {
        self.at_contextual("Fn") && self.nth_at(1, TokenKind::LeftParen)
    }

    fn parse_dynamic_type(&mut self) -> Option<CompletedMarker> {
        let dynamic = self.start();
        self.bump();
        self.parse_path_type();
        Some(self.complete(dynamic, SyntaxKind::DynType))
    }

    fn parse_path_type(&mut self) -> Option<CompletedMarker> {
        let path = self.start();
        self.expect_identifier();
        while self.at(TokenKind::ColonColon) && self.nth_at(1, TokenKind::Identifier) {
            self.bump();
            self.bump();
        }
        if self.at(TokenKind::LeftBracket) {
            self.parse_type_argument_list();
        }
        Some(self.complete(path, SyntaxKind::PathType))
    }

    fn parse_parenthesized_type(&mut self) -> Option<CompletedMarker> {
        let parenthesized = self.start();
        self.bump();
        self.skip_newlines();
        if self.at(TokenKind::RightParen) {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            self.bump();
            return Some(self.complete(parenthesized, SyntaxKind::TupleType));
        }

        self.parse_type();
        self.skip_newlines();
        if self.eat(TokenKind::Comma) {
            self.skip_newlines();
            if !self.at(TokenKind::RightParen) {
                self.parse_type_list(TokenKind::RightParen);
            }
            self.expect_closing(TokenKind::RightParen);
            Some(self.complete(parenthesized, SyntaxKind::TupleType))
        } else {
            self.expect_closing(TokenKind::RightParen);
            Some(self.complete(parenthesized, SyntaxKind::TypeExpr))
        }
    }

    fn parse_function_type(&mut self) -> Option<CompletedMarker> {
        let function = self.start();
        self.bump();
        self.expect(TokenKind::LeftParen);
        self.skip_newlines();
        if !self.at(TokenKind::RightParen) {
            self.parse_type_list(TokenKind::RightParen);
        }
        self.expect_closing(TokenKind::RightParen);
        self.expect(TokenKind::Arrow);
        self.parse_type();
        Some(self.complete(function, SyntaxKind::FnType))
    }

    fn parse_type_list(&mut self, closing: TokenKind) {
        let types = self.start();
        loop {
            self.parse_type();
            self.skip_newlines();
            if !self.eat(TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
            if self.at(closing) {
                break;
            }
        }
        self.complete(types, SyntaxKind::TypeList);
    }
}

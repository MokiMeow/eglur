use crate::event::CompletedMarker;
use crate::{SyntaxDiagnosticKind, SyntaxKind, TokenKind};

use super::Parser;

impl Parser<'_> {
    pub(super) fn parse_pattern(&mut self) -> Option<CompletedMarker> {
        if !self.enter_recursive() {
            return None;
        }
        let pattern = self.start();
        let mut parsed = self.parse_pattern_atom();
        if self.at(TokenKind::Pipe)
            && let Some(first) = parsed
        {
            let alternatives = self.precede(first);
            while self.eat(TokenKind::Pipe) {
                self.parse_pattern_atom();
            }
            parsed = Some(self.complete(alternatives, SyntaxKind::OrPattern));
        }
        if parsed.is_none() && !self.halted() {
            self.error_current(SyntaxDiagnosticKind::ParseExpectedPattern);
        }
        let completed = self.complete(pattern, SyntaxKind::Pattern);
        self.leave_recursive();
        Some(completed)
    }

    fn parse_pattern_atom(&mut self) -> Option<CompletedMarker> {
        match self.current_kind() {
            Some(TokenKind::Underscore) => {
                let wildcard = self.start();
                self.bump();
                Some(self.complete(wildcard, SyntaxKind::WildcardPattern))
            }
            Some(
                TokenKind::Integer
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::TrueKw
                | TokenKind::FalseKw,
            ) => {
                let literal = self.start();
                self.bump();
                Some(self.complete(literal, SyntaxKind::LiteralPattern))
            }
            Some(TokenKind::Identifier) => self.parse_named_pattern(),
            Some(TokenKind::LeftParen) => self.parse_parenthesized_pattern(),
            Some(TokenKind::LeftBrace) => self.parse_struct_pattern(false),
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseExpectedPattern);
                None
            }
        }
    }

    fn parse_named_pattern(&mut self) -> Option<CompletedMarker> {
        let pattern = self.start();
        let qualified = self.path_is_qualified();
        self.parse_path(SyntaxKind::Path);

        if self.eat(TokenKind::At) {
            if qualified {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
            }
            if self.enter_recursive() {
                self.parse_pattern_atom();
                self.leave_recursive();
            }
            return Some(self.complete(pattern, SyntaxKind::AliasPattern));
        }

        match self.current_kind() {
            Some(TokenKind::LeftParen) => {
                self.parse_positional_pattern_payload();
                Some(self.complete(pattern, SyntaxKind::VariantPattern))
            }
            Some(TokenKind::LeftBrace) if qualified => {
                self.parse_field_pattern_payload();
                Some(self.complete(pattern, SyntaxKind::VariantPattern))
            }
            Some(TokenKind::LeftBrace) => {
                self.parse_field_pattern_payload();
                Some(self.complete(pattern, SyntaxKind::StructPattern))
            }
            _ if qualified => Some(self.complete(pattern, SyntaxKind::VariantPattern)),
            _ => Some(self.complete(pattern, SyntaxKind::BindingPattern)),
        }
    }

    fn parse_parenthesized_pattern(&mut self) -> Option<CompletedMarker> {
        let pattern = self.start();
        self.bump();
        self.skip_newlines();
        if self.eat(TokenKind::RightParen) {
            return Some(self.complete(pattern, SyntaxKind::UnitPattern));
        }
        if self.at(TokenKind::Range) {
            self.parse_pattern_list(TokenKind::RightParen);
            self.expect_closing(TokenKind::RightParen);
            return Some(self.complete(pattern, SyntaxKind::TuplePattern));
        }

        self.parse_pattern();
        self.skip_newlines();
        if self.eat(TokenKind::Comma) {
            self.skip_newlines();
            if !self.at(TokenKind::RightParen) {
                self.parse_pattern_list(TokenKind::RightParen);
            }
            self.expect_closing(TokenKind::RightParen);
            Some(self.complete(pattern, SyntaxKind::TuplePattern))
        } else {
            self.expect_closing(TokenKind::RightParen);
            Some(self.complete(pattern, SyntaxKind::GroupPattern))
        }
    }

    fn parse_struct_pattern(&mut self, has_path: bool) -> Option<CompletedMarker> {
        let pattern = self.start();
        if has_path {
            self.parse_path(SyntaxKind::Path);
        }
        self.parse_field_pattern_payload();
        Some(self.complete(pattern, SyntaxKind::StructPattern))
    }

    fn parse_positional_pattern_payload(&mut self) {
        self.bump();
        self.skip_newlines();
        if !self.at(TokenKind::RightParen) {
            self.parse_pattern_list(TokenKind::RightParen);
        }
        self.expect_closing(TokenKind::RightParen);
    }

    fn parse_pattern_list(&mut self, closing: TokenKind) {
        let patterns = self.start();
        let mut saw_rest = false;
        loop {
            if self.eat(TokenKind::Range) {
                if saw_rest {
                    self.error_current(SyntaxDiagnosticKind::ParseInvalidRestPosition);
                }
                saw_rest = true;
            } else {
                if saw_rest {
                    self.error_current(SyntaxDiagnosticKind::ParseInvalidRestPosition);
                }
                self.parse_pattern();
            }
            self.skip_newlines();
            if !self.eat(TokenKind::Comma) {
                break;
            }
            self.skip_newlines();
            if self.at(closing) {
                break;
            }
        }
        self.complete(patterns, SyntaxKind::PatternList);
    }

    fn parse_field_pattern_payload(&mut self) {
        let fields = self.start();
        self.expect(TokenKind::LeftBrace);
        self.skip_newlines();
        let mut saw_rest = false;
        if !self.at(TokenKind::RightBrace) {
            loop {
                let field = self.start();
                if self.eat(TokenKind::Range) {
                    if saw_rest {
                        self.error_current(SyntaxDiagnosticKind::ParseInvalidRestPosition);
                    }
                    saw_rest = true;
                } else {
                    if saw_rest {
                        self.error_current(SyntaxDiagnosticKind::ParseInvalidRestPosition);
                    }
                    self.expect_identifier();
                    if self.eat(TokenKind::Colon) {
                        self.parse_pattern();
                    }
                }
                self.complete(field, SyntaxKind::FieldPattern);
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
        self.complete(fields, SyntaxKind::FieldPatternList);
    }

    fn path_is_qualified(&self) -> bool {
        self.at(TokenKind::Identifier)
            && self.nth_at(1, TokenKind::ColonColon)
            && self.nth_at(2, TokenKind::Identifier)
    }
}

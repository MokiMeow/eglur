use crate::event::CompletedMarker;
use crate::{SyntaxDiagnosticKind, SyntaxKind, TokenKind};

use super::Parser;

impl Parser<'_> {
    pub(super) fn at_tensor_schema_start(&mut self) -> bool {
        matches!(
            self.current_text(),
            Some("view" | "rearrange" | "inflate" | "reduce" | "contract" | "einop")
        ) && self.nth_at(1, TokenKind::LeftBrace)
            && self.brace_has_top_level_arrow(self.logical_position() + 1)
    }

    pub(super) fn parse_tensor_schema(&mut self) -> Option<CompletedMarker> {
        let family = self.current_text()?.to_owned();
        let schema = self.start();
        self.bump();
        if !self.expect(TokenKind::LeftBrace) {
            return Some(self.complete(schema, SyntaxKind::TensorSchemaExpr));
        }
        self.skip_newlines();

        let left_limit = (family == "reduce").then_some(1);
        let left_count = self.parse_transform_lists(TokenKind::Arrow, left_limit);
        self.skip_newlines();
        if !self.expect(TokenKind::Arrow) {
            return Some(self.complete(schema, SyntaxKind::TensorSchemaExpr));
        }
        self.skip_newlines();
        let right_limit = matches!(family.as_str(), "reduce" | "contract").then_some(1);
        let right_count = self.parse_transform_lists(TokenKind::RightBrace, right_limit);
        self.skip_newlines();

        if !self.halted()
            && ((family == "reduce" && left_count != 1)
                || (family == "reduce" && right_count != 1)
                || (family == "contract" && right_count != 1))
        {
            self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
        }
        self.expect_closing(TokenKind::RightBrace);
        Some(self.complete(schema, SyntaxKind::TensorSchemaExpr))
    }

    pub(super) fn parse_reduce_by_suffix(&mut self, receiver: CompletedMarker) -> CompletedMarker {
        let candidate = self.precede(receiver);
        self.expect(TokenKind::Dot);
        self.expect_identifier();
        self.expect(TokenKind::LeftParen);
        let arguments = self.start();

        if self.reduce_by_arguments_form_phase_candidate() {
            self.skip_newlines();
            loop {
                self.parse_reduce_phase();
                self.skip_newlines();
                if !self.eat(TokenKind::Comma) {
                    break;
                }
                self.skip_newlines();
                if self.at(TokenKind::RightParen) {
                    break;
                }
            }
        } else {
            self.parse_call_arguments_until_close();
        }

        self.complete(arguments, SyntaxKind::ReduceByArgumentList);
        self.expect_closing(TokenKind::RightParen);
        self.complete(candidate, SyntaxKind::ReduceByCandidateExpr)
    }

    fn parse_transform_lists(&mut self, terminator: TokenKind, limit: Option<usize>) -> usize {
        let lists = self.start();
        let mut count = 0;
        loop {
            count += 1;
            self.parse_transform_axis_list(terminator);
            if self.halted() || !self.at(TokenKind::Comma) {
                break;
            }
            if limit.is_some_and(|maximum| count >= maximum) {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                break;
            }
            self.bump();
        }
        self.complete(lists, SyntaxKind::TransformOperandList);
        count
    }

    fn parse_transform_axis_list(&mut self, terminator: TokenKind) {
        let list = self.start();
        while !self.at(terminator) && !self.at(TokenKind::Comma) && !self.at_eof() && !self.halted()
        {
            if self.at(TokenKind::Newline) {
                let mut distance = 1;
                while self.nth_at(distance, TokenKind::Newline) {
                    distance += 1;
                }
                if self.nth_at(distance, terminator) {
                    break;
                }
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                break;
            }
            if matches!(
                self.current_kind(),
                Some(TokenKind::AxisPackAnonymous | TokenKind::AxisPackNamed)
            ) {
                let pack = self.start();
                self.bump();
                self.complete(pack, SyntaxKind::AxisAtom);
            } else {
                self.parse_axis_expression(1);
            }
        }
        self.complete(list, SyntaxKind::TransformAxisList);
    }

    fn parse_axis_expression(&mut self, minimum_precedence: u8) -> Option<CompletedMarker> {
        if !self.enter_recursive() {
            return None;
        }
        let mut left = self.parse_axis_atom();
        while let Some(completed) = left {
            let precedence = match self.current_kind() {
                Some(TokenKind::Plus) => 1,
                Some(TokenKind::Star) => 2,
                _ => break,
            };
            if precedence < minimum_precedence {
                break;
            }
            let binary = self.precede(completed);
            self.bump();
            self.parse_axis_expression(precedence + 1);
            left = Some(self.complete(binary, SyntaxKind::AxisBinaryExpr));
            if self.halted() {
                break;
            }
        }
        self.leave_recursive();
        left
    }

    fn parse_axis_atom(&mut self) -> Option<CompletedMarker> {
        let atom = self.start();
        match self.current_kind() {
            Some(TokenKind::Identifier | TokenKind::Integer) => self.bump(),
            Some(TokenKind::LeftParen) => {
                self.bump();
                self.parse_axis_expression(1);
                self.expect_closing(TokenKind::RightParen);
            }
            _ => {
                self.error_current(SyntaxDiagnosticKind::ParseUnexpectedToken);
                return Some(self.complete(atom, SyntaxKind::AxisAtom));
            }
        }
        Some(self.complete(atom, SyntaxKind::AxisAtom))
    }

    fn parse_reduce_phase(&mut self) {
        let phase = self.start();
        self.expect(TokenKind::LeftParen);
        self.skip_newlines();
        if self.eat(TokenKind::LeftParen) {
            self.expect_identifier();
            while self.at(TokenKind::Identifier) && !self.halted() {
                self.bump();
            }
            self.expect_closing(TokenKind::RightParen);
        } else {
            self.expect_identifier();
        }
        self.skip_newlines();
        self.expect(TokenKind::Comma);
        self.skip_newlines();
        self.parse_expression(false, true);
        self.skip_newlines();
        self.expect_closing(TokenKind::RightParen);
        self.complete(phase, SyntaxKind::ReduceByPhase);
    }

    fn reduce_by_arguments_form_phase_candidate(&mut self) -> bool {
        let mut position = self.logical_position();
        while self.kind_at_logical(position) == Some(TokenKind::Newline) {
            position += 1;
        }
        let mut phase_count = 0;

        loop {
            if self.kind_at_logical(position) != Some(TokenKind::LeftParen) {
                return false;
            }
            let Some(close) = self.matching_delimiter(position) else {
                return false;
            };
            if !self.tokens_form_reduce_phase(position, close) {
                return false;
            }
            phase_count += 1;
            position = close + 1;
            while self.kind_at_logical(position) == Some(TokenKind::Newline) {
                position += 1;
            }

            match self.kind_at_logical(position) {
                Some(TokenKind::RightParen) => return phase_count > 0,
                Some(TokenKind::Comma) => {
                    position += 1;
                    while self.kind_at_logical(position) == Some(TokenKind::Newline) {
                        position += 1;
                    }
                    if self.kind_at_logical(position) == Some(TokenKind::RightParen) {
                        return phase_count > 0;
                    }
                }
                _ => return false,
            }
        }
    }

    fn tokens_form_reduce_phase(&mut self, opening: usize, close: usize) -> bool {
        let mut position = opening + 1;
        let mut comma = None;
        while position < close {
            match self.kind_at_logical(position) {
                Some(TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace) => {
                    let Some(nested_close) = self.matching_delimiter(position) else {
                        return false;
                    };
                    position = nested_close + 1;
                }
                Some(TokenKind::Comma) => {
                    if comma.replace(position).is_some() {
                        return false;
                    }
                    position += 1;
                }
                Some(_) => position += 1,
                None => return false,
            }
        }
        let Some(comma) = comma else {
            return false;
        };

        let mut axes_start = opening + 1;
        while self.kind_at_logical(axes_start) == Some(TokenKind::Newline) {
            axes_start += 1;
        }
        let mut axes_end = comma;
        while axes_end > axes_start
            && self.kind_at_logical(axes_end - 1) == Some(TokenKind::Newline)
        {
            axes_end -= 1;
        }
        if !self.tokens_form_reduce_phase_axes(axes_start, axes_end) {
            return false;
        }

        (comma + 1..close)
            .any(|position| self.kind_at_logical(position) != Some(TokenKind::Newline))
    }

    fn tokens_form_reduce_phase_axes(&mut self, start: usize, end: usize) -> bool {
        if end == start + 1 {
            return self.kind_at_logical(start) == Some(TokenKind::Identifier);
        }
        if self.kind_at_logical(start) != Some(TokenKind::LeftParen)
            || self.matching_delimiter(start) != Some(end.saturating_sub(1))
        {
            return false;
        }

        let mut identifier_count = 0;
        for position in start + 1..end - 1 {
            if self.kind_at_logical(position) != Some(TokenKind::Identifier) {
                return false;
            }
            identifier_count += 1;
        }
        identifier_count >= 2
    }

    fn brace_has_top_level_arrow(&mut self, opening: usize) -> bool {
        let Some(close) = self.matching_delimiter(opening) else {
            return false;
        };
        let mut nested = 0_u32;
        for position in opening + 1..close {
            match self.kind_at_logical(position) {
                Some(TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace) => {
                    nested += 1;
                }
                Some(TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace) => {
                    nested = nested.saturating_sub(1);
                }
                Some(TokenKind::Arrow) if nested == 0 => return true,
                _ => {}
            }
        }
        false
    }
}

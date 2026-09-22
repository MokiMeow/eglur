use eglur_source::{ByteOffset, ByteRange, FileSpan, ValidatedSource};
use unicode_ident::{is_xid_continue, is_xid_start};

use crate::{SyntaxDiagnostic, SyntaxDiagnosticKind, Token, TokenKind};

const MULTI_TOKENS: &[(&str, TokenKind)] = &[
    ("...", TokenKind::AxisPackAnonymous),
    ("..=", TokenKind::RangeInclusive),
    ("#[", TokenKind::HashLeftBracket),
    ("->", TokenKind::Arrow),
    ("=>", TokenKind::FatArrow),
    ("::", TokenKind::ColonColon),
    ("..", TokenKind::Range),
    ("/.", TokenKind::FractionalDivide),
    ("==", TokenKind::EqualEqual),
    ("!=", TokenKind::NotEqual),
    ("<=", TokenKind::LessEqual),
    (">=", TokenKind::GreaterEqual),
    ("&&", TokenKind::LogicalAnd),
    ("||", TokenKind::LogicalOr),
];

/// Lossless physical token buffer and terminal lexical diagnostic, if any.
#[derive(Clone, Debug)]
pub struct Lexed<'source> {
    source: ValidatedSource<'source>,
    tokens: Box<[Token]>,
    diagnostics: Box<[SyntaxDiagnostic]>,
}

impl<'source> Lexed<'source> {
    /// Returns the exact validated source represented by this buffer.
    #[must_use]
    pub const fn source(&self) -> ValidatedSource<'source> {
        self.source
    }

    /// Returns every non-empty physical token in source order.
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Returns the terminal lexical diagnostic, when source scanning failed.
    #[must_use]
    pub fn diagnostics(&self) -> &[SyntaxDiagnostic] {
        &self.diagnostics
    }

    /// Returns the exact text covered by one token from this buffer.
    ///
    /// A token from a different source buffer has no text in this buffer.
    #[must_use]
    pub fn text(&self, token: Token) -> Option<&'source str> {
        if token.span().file() != self.source.id() {
            return None;
        }
        let range = token.range();
        self.source
            .text()
            .get(range.start().as_usize()..range.end().as_usize())
    }

    /// Reports whether lexing consumed the complete source without a diagnostic.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Partitions one validated source into exact physical tokens.
#[must_use]
pub fn lex(source: ValidatedSource<'_>) -> Lexed<'_> {
    Lexer::new(source).run()
}

/// Checks one ordinary identifier, excluding reserved keywords, `_`, and bang names.
#[must_use]
pub fn is_identifier_spelling(text: &str) -> bool {
    let mut characters = text.chars();
    characters.next().is_some_and(is_identifier_start)
        && characters.all(is_identifier_continue)
        && identifier_kind(text) == TokenKind::Identifier
}

struct Lexer<'source> {
    source: ValidatedSource<'source>,
    index: usize,
    tokens: Vec<Token>,
    diagnostic: Option<SyntaxDiagnostic>,
    tuple_index_context: bool,
    alias_binder_context: bool,
}

impl<'source> Lexer<'source> {
    fn new(source: ValidatedSource<'source>) -> Self {
        Self {
            source,
            index: 0,
            tokens: Vec::with_capacity(source.text().len().min(4096)),
            diagnostic: None,
            tuple_index_context: false,
            alias_binder_context: false,
        }
    }

    fn run(mut self) -> Lexed<'source> {
        if self.source.has_leading_bom() {
            self.index = 3;
            self.emit(TokenKind::Bom, 0, 3);
        }

        while !self.at_end() && self.diagnostic.is_none() {
            let start = self.index;
            let character = self.peek_char().expect("non-empty source tail");
            if matches!(character, ' ' | '\t' | '\u{000c}') {
                self.consume_whitespace();
                self.emit(TokenKind::Whitespace, start, self.index);
            } else if matches!(character, '\r' | '\n') {
                self.consume_newline();
                self.emit(TokenKind::Newline, start, self.index);
            } else if self.starts_with("//") {
                self.consume_line_comment();
                self.emit(TokenKind::LineComment, start, self.index);
            } else if self.starts_with("/*") {
                self.consume_block_comment(start);
            } else if self.starts_with("f\"") {
                self.consume_string(start, true);
            } else if character == '"' {
                self.consume_string(start, false);
            } else if character.is_ascii_digit() {
                self.consume_number(start, self.tuple_index_context);
            } else if is_identifier_start(character) {
                self.consume_identifier(start);
            } else if character == '\'' {
                self.consume_axis_name(start);
            } else if character == '#' && !self.starts_with("#[") {
                self.consume_axis_index(start);
            } else if !character.is_ascii() {
                self.advance_char();
                self.fail(
                    unsupported_unicode_kind(character),
                    start,
                    self.index,
                    start,
                );
            } else {
                self.consume_operator_or_delimiter(start);
            }
        }

        Lexed {
            source: self.source,
            tokens: self.tokens.into_boxed_slice(),
            diagnostics: self.diagnostic.into_iter().collect(),
        }
    }

    fn text(&self) -> &'source str {
        self.source.text()
    }

    fn at_end(&self) -> bool {
        self.index >= self.text().len()
    }

    fn starts_with(&self, text: &str) -> bool {
        self.text()[self.index..].starts_with(text)
    }

    fn peek_char(&self) -> Option<char> {
        self.text()[self.index..].chars().next()
    }

    fn peek_char_after(&self, byte_count: usize) -> Option<char> {
        self.text().get(self.index + byte_count..)?.chars().next()
    }

    fn advance_char(&mut self) -> Option<char> {
        let character = self.peek_char()?;
        self.index += character.len_utf8();
        Some(character)
    }

    fn consume_whitespace(&mut self) {
        while self
            .peek_char()
            .is_some_and(|character| matches!(character, ' ' | '\t' | '\u{000c}'))
        {
            self.advance_char();
        }
    }

    fn consume_newline(&mut self) {
        if self.starts_with("\r\n") {
            self.index += 2;
        } else {
            self.index += 1;
        }
    }

    fn consume_line_comment(&mut self) {
        self.index += 2;
        while self
            .peek_char()
            .is_some_and(|character| !matches!(character, '\r' | '\n'))
        {
            self.advance_char();
        }
    }

    fn consume_block_comment(&mut self, start: usize) {
        let mut scan = start + 2;
        let mut close = None;
        while scan < self.text().len() {
            let tail = &self.text()[scan..];
            if tail.starts_with("/*") {
                if scan > start {
                    self.emit(TokenKind::BlockComment, start, scan);
                }
                self.index = scan + 2;
                self.fail(
                    SyntaxDiagnosticKind::LexNestedBlockComment,
                    scan,
                    self.index,
                    scan,
                );
                return;
            }
            if tail.starts_with("*/") {
                close = Some(scan + 2);
                break;
            }
            let character = tail.chars().next().expect("non-empty block comment tail");
            scan += character.len_utf8();
        }

        let Some(end) = close else {
            self.index = self.text().len();
            self.fail(
                SyntaxDiagnosticKind::LexUnterminatedBlockComment,
                start,
                self.index,
                start,
            );
            return;
        };

        self.partition_valid_block_comment(start, end);
        self.index = end;
    }

    fn partition_valid_block_comment(&mut self, start: usize, end: usize) {
        let mut segment_start = start;
        let mut scan = start;
        while scan < end {
            let tail = &self.text()[scan..end];
            let newline_len = if tail.starts_with("\r\n") {
                2
            } else if tail.starts_with(['\r', '\n']) {
                1
            } else {
                scan += tail
                    .chars()
                    .next()
                    .expect("non-empty block comment tail")
                    .len_utf8();
                continue;
            };
            if segment_start < scan {
                self.emit(TokenKind::BlockComment, segment_start, scan);
            }
            self.emit(TokenKind::Newline, scan, scan + newline_len);
            scan += newline_len;
            segment_start = scan;
        }
        if segment_start < end {
            self.emit(TokenKind::BlockComment, segment_start, end);
        }
    }

    fn consume_string(&mut self, start: usize, formatted: bool) {
        if formatted {
            self.index += 1;
        }
        self.index += 1;
        while !self.at_end() {
            match self.peek_char().expect("non-empty string tail") {
                '"' => {
                    self.index += 1;
                    self.emit(
                        if formatted {
                            TokenKind::FormatString
                        } else {
                            TokenKind::String
                        },
                        start,
                        self.index,
                    );
                    return;
                }
                '\r' | '\n' => {
                    self.fail(
                        SyntaxDiagnosticKind::LexUnterminatedString,
                        start,
                        self.index,
                        start,
                    );
                    return;
                }
                '\\' => {
                    if !self.consume_escape(start) {
                        return;
                    }
                }
                _ => {
                    self.advance_char();
                }
            }
        }
        self.fail(
            SyntaxDiagnosticKind::LexUnterminatedString,
            start,
            self.index,
            start,
        );
    }

    fn consume_escape(&mut self, string_start: usize) -> bool {
        let escape_start = self.index;
        self.index += 1;
        let Some(escaped) = self.peek_char() else {
            self.fail(
                SyntaxDiagnosticKind::LexInvalidEscape,
                string_start,
                self.index,
                escape_start,
            );
            return false;
        };
        if matches!(escaped, '\\' | '"' | 'n' | 'r' | 't') {
            self.advance_char();
            return true;
        }
        if escaped != 'u' {
            self.advance_char();
            self.fail(
                SyntaxDiagnosticKind::LexInvalidEscape,
                string_start,
                self.index,
                escape_start,
            );
            return false;
        }

        self.index += 1;
        if self.peek_char() != Some('{') {
            self.fail(
                SyntaxDiagnosticKind::LexInvalidEscape,
                string_start,
                self.index,
                escape_start,
            );
            return false;
        }
        self.index += 1;
        let digits_start = self.index;
        while self
            .peek_char()
            .is_some_and(|character| character.is_ascii_hexdigit())
        {
            self.index += 1;
        }
        let digit_count = self.index - digits_start;
        let has_closing_brace = self.peek_char() == Some('}');
        if has_closing_brace {
            self.index += 1;
        } else if !self.at_end() {
            self.advance_char();
        }

        let scalar = if (1..=6).contains(&digit_count) && has_closing_brace {
            u32::from_str_radix(&self.text()[digits_start..digits_start + digit_count], 16)
                .ok()
                .and_then(char::from_u32)
        } else {
            None
        };
        if scalar.is_none() {
            self.fail(
                SyntaxDiagnosticKind::LexInvalidEscape,
                string_start,
                self.index,
                escape_start,
            );
            return false;
        }
        true
    }

    fn consume_identifier(&mut self, start: usize) {
        self.advance_char();
        while self.peek_char().is_some_and(is_identifier_continue) {
            self.advance_char();
        }
        let text = &self.text()[start..self.index];
        let mut kind = identifier_kind(text);
        if kind == TokenKind::Identifier && self.peek_char() == Some('!') {
            self.index += 1;
            kind = TokenKind::BangIdentifier;
        }
        self.emit(kind, start, self.index);
    }

    fn consume_axis_name(&mut self, start: usize) {
        self.index += 1;
        if !self.peek_char().is_some_and(is_identifier_start) {
            if let Some(character) = self.peek_char().filter(|value| !value.is_ascii()) {
                let diagnostic_start = self.index;
                self.advance_char();
                self.fail(
                    unsupported_unicode_kind(character),
                    start,
                    self.index,
                    diagnostic_start,
                );
                return;
            }
            self.fail(
                SyntaxDiagnosticKind::LexInvalidChar,
                start,
                self.index,
                start,
            );
            return;
        }
        self.advance_char();
        while self.peek_char().is_some_and(is_identifier_continue) {
            self.advance_char();
        }
        self.emit(TokenKind::AxisName, start, self.index);
    }

    fn consume_axis_index(&mut self, start: usize) {
        self.index += 1;
        if !self
            .peek_char()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.fail(
                SyntaxDiagnosticKind::LexInvalidChar,
                start,
                self.index,
                start,
            );
            return;
        }
        if !self.consume_digit_sequence(start) {
            return;
        }
        if self.peek_char().is_some_and(is_identifier_continue) {
            self.consume_invalid_number_suffix(start);
            return;
        }
        self.emit(TokenKind::AxisIndex, start, self.index);
    }

    fn consume_number(&mut self, start: usize, tuple_index: bool) {
        if !self.consume_digit_sequence(start) {
            return;
        }
        let mut kind = if tuple_index {
            TokenKind::TupleIndex
        } else {
            TokenKind::Integer
        };

        if tuple_index {
            if self.peek_char().is_some_and(is_identifier_start) {
                self.consume_invalid_number_suffix(start);
                return;
            }
        } else if self.peek_char() == Some('.')
            && self
                .peek_char_after(1)
                .is_some_and(|character| character.is_ascii_digit())
        {
            kind = TokenKind::Float;
            self.index += 1;
            if !self.consume_digit_sequence(start) || !self.consume_exponent(start) {
                return;
            }
            if self.peek_char() == Some('.')
                && self
                    .peek_char_after(1)
                    .is_some_and(|character| character.is_ascii_digit())
            {
                self.index += 1;
                if !self.consume_digit_sequence(start) {
                    return;
                }
                self.fail(
                    SyntaxDiagnosticKind::LexInvalidNumber,
                    start,
                    self.index,
                    start,
                );
                return;
            }
        } else if self
            .peek_char()
            .is_some_and(|character| matches!(character, 'e' | 'E'))
        {
            kind = TokenKind::Float;
            if !self.consume_exponent(start) {
                return;
            }
        }

        if self.peek_char().is_some_and(is_identifier_start) {
            self.consume_invalid_number_suffix(start);
            return;
        }
        self.emit(kind, start, self.index);
    }

    fn consume_digit_sequence(&mut self, number_start: usize) -> bool {
        let mut previous_was_underscore = false;
        let mut consumed_digit = false;
        loop {
            match self.peek_char() {
                Some(character) if character.is_ascii_digit() => {
                    consumed_digit = true;
                    previous_was_underscore = false;
                    self.index += 1;
                }
                Some('_') => {
                    self.index += 1;
                    if !consumed_digit || previous_was_underscore {
                        self.fail(
                            SyntaxDiagnosticKind::LexInvalidNumber,
                            number_start,
                            self.index,
                            number_start,
                        );
                        return false;
                    }
                    previous_was_underscore = true;
                }
                _ => break,
            }
        }
        if previous_was_underscore {
            self.fail(
                SyntaxDiagnosticKind::LexInvalidNumber,
                number_start,
                self.index,
                number_start,
            );
            return false;
        }
        true
    }

    fn consume_exponent(&mut self, number_start: usize) -> bool {
        if !self
            .peek_char()
            .is_some_and(|character| matches!(character, 'e' | 'E'))
        {
            return true;
        }
        self.index += 1;
        if self
            .peek_char()
            .is_some_and(|character| matches!(character, '+' | '-'))
        {
            self.index += 1;
        }
        if !self
            .peek_char()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.fail(
                SyntaxDiagnosticKind::LexInvalidNumber,
                number_start,
                self.index,
                number_start,
            );
            return false;
        }
        self.consume_digit_sequence(number_start)
    }

    fn consume_invalid_number_suffix(&mut self, start: usize) {
        while self.peek_char().is_some_and(is_identifier_continue) {
            self.advance_char();
        }
        self.fail(
            SyntaxDiagnosticKind::LexInvalidNumber,
            start,
            self.index,
            start,
        );
    }

    fn consume_operator_or_delimiter(&mut self, start: usize) {
        if self.peek_char() == Some('@') {
            self.index += 1;
            if self.alias_binder_context {
                self.emit(TokenKind::At, start, self.index);
            } else {
                self.fail(
                    SyntaxDiagnosticKind::LexInvalidChar,
                    start,
                    self.index,
                    start,
                );
            }
            return;
        }
        if self.starts_with("...") {
            self.index += 3;
            if self.peek_char().is_some_and(is_identifier_start) {
                self.advance_char();
                while self.peek_char().is_some_and(is_identifier_continue) {
                    self.advance_char();
                }
                self.emit(TokenKind::AxisPackNamed, start, self.index);
            } else {
                self.emit(TokenKind::AxisPackAnonymous, start, self.index);
            }
            return;
        }
        for &(text, kind) in &MULTI_TOKENS[1..] {
            if self.starts_with(text) {
                self.index += text.len();
                self.emit(kind, start, self.index);
                return;
            }
        }
        let character = self.peek_char().expect("non-empty source tail");
        if let Some(kind) = single_token(character) {
            self.index += 1;
            self.emit(kind, start, self.index);
            return;
        }
        self.index += 1;
        self.fail(
            SyntaxDiagnosticKind::LexInvalidChar,
            start,
            self.index,
            start,
        );
    }

    fn emit(&mut self, kind: TokenKind, start: usize, end: usize) {
        if start == end {
            return;
        }
        let range = byte_range(start, end);
        self.tokens
            .push(Token::new(kind, FileSpan::new(self.source.id(), range)));
        match kind {
            TokenKind::Dot => self.tuple_index_context = true,
            TokenKind::Bom
            | TokenKind::Whitespace
            | TokenKind::LineComment
            | TokenKind::BlockComment => {}
            _ => self.tuple_index_context = false,
        }
        match kind {
            TokenKind::Identifier => self.alias_binder_context = true,
            TokenKind::Bom
            | TokenKind::Whitespace
            | TokenKind::LineComment
            | TokenKind::BlockComment => {}
            _ => self.alias_binder_context = false,
        }
    }

    fn fail(
        &mut self,
        kind: SyntaxDiagnosticKind,
        token_start: usize,
        diagnostic_end: usize,
        diagnostic_start: usize,
    ) {
        if token_start < self.index {
            self.emit(TokenKind::Error, token_start, self.index);
        }
        if self.index < self.text().len() {
            self.emit(TokenKind::Raw, self.index, self.text().len());
            self.index = self.text().len();
        }
        self.diagnostic = Some(SyntaxDiagnostic::new(
            kind,
            FileSpan::new(
                self.source.id(),
                byte_range(diagnostic_start, diagnostic_end),
            ),
        ));
    }
}

fn byte_range(start: usize, end: usize) -> ByteRange {
    let start = ByteOffset::new(u32::try_from(start).expect("validated source is u32-bounded"));
    let end = ByteOffset::new(u32::try_from(end).expect("validated source is u32-bounded"));
    ByteRange::new(start, end).expect("lexer ranges are emitted in source order")
}

fn identifier_kind(text: &str) -> TokenKind {
    if text == "_" {
        TokenKind::Underscore
    } else {
        TokenKind::keyword(text).unwrap_or(TokenKind::Identifier)
    }
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn unsupported_unicode_kind(character: char) -> SyntaxDiagnosticKind {
    if is_xid_start(character) || is_xid_continue(character) {
        SyntaxDiagnosticKind::LexInvalidIdentifierChar
    } else {
        SyntaxDiagnosticKind::LexInvalidChar
    }
}

fn single_token(character: char) -> Option<TokenKind> {
    Some(match character {
        '?' => TokenKind::Question,
        '=' => TokenKind::Equal,
        '<' => TokenKind::Less,
        '>' => TokenKind::Greater,
        '+' => TokenKind::Plus,
        '-' => TokenKind::Minus,
        '*' => TokenKind::Star,
        '/' => TokenKind::Slash,
        '%' => TokenKind::Percent,
        '!' => TokenKind::Bang,
        '&' => TokenKind::Ampersand,
        '|' => TokenKind::Pipe,
        ':' => TokenKind::Colon,
        ',' => TokenKind::Comma,
        '.' => TokenKind::Dot,
        '(' => TokenKind::LeftParen,
        ')' => TokenKind::RightParen,
        '{' => TokenKind::LeftBrace,
        '}' => TokenKind::RightBrace,
        '[' => TokenKind::LeftBracket,
        ']' => TokenKind::RightBracket,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use eglur_source::{ByteOffset, FileId, SourceFile};

    use super::{is_identifier_spelling, lex};
    use crate::{SyntaxDiagnosticKind, TokenKind};

    fn lex_text(text: &str) -> super::Lexed<'_> {
        let source = Box::leak(Box::new(SourceFile::new(
            FileId::new(1),
            text.as_bytes().to_vec(),
        )));
        lex(source.validate().expect("test source should validate"))
    }

    #[test]
    fn identifier_spelling_agrees_with_complete_ordinary_identifier_tokens() {
        for text in [
            "Red",
            "_name",
            "a123",
            "self",
            "super",
            "closed",
            "view",
            "Fn",
            "Never",
            "",
            "_",
            "fn",
            "match",
            "own",
            "drop_own",
            "Red!",
            "123",
            "a b",
            "a-b",
            "a\n",
            "a\0",
            "a::b",
            "\u{03bb}",
            "a\u{03bb}",
        ] {
            let source = SourceFile::new(FileId::new(1), text.as_bytes().to_vec());
            let tokens = lex(source.validate().unwrap());
            let expected = tokens.diagnostics().is_empty()
                && matches!(tokens.tokens(), [token] if token.kind() == TokenKind::Identifier);
            assert_eq!(is_identifier_spelling(text), expected, "{text:?}");
        }
    }

    fn significant(lexed: &super::Lexed<'_>) -> Vec<(TokenKind, String)> {
        lexed
            .tokens()
            .iter()
            .filter(|token| !token.kind().is_horizontal_trivia())
            .map(|token| {
                (
                    token.kind(),
                    lexed
                        .text(*token)
                        .expect("token should belong to its lexical buffer")
                        .to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn every_token_reconstructs_the_exact_source() {
        let source = "\u{feff}fn main!() {\r\n\tlet x = pair./* a\nb */0 // c\n}\n";
        let lexed = lex_text(source);
        let reconstructed = lexed
            .tokens()
            .iter()
            .map(|token| {
                lexed
                    .text(*token)
                    .expect("token should belong to its lexical buffer")
            })
            .collect::<String>();

        assert!(lexed.is_success());
        assert_eq!(reconstructed, source);
        assert_eq!(lexed.tokens()[0].kind(), TokenKind::Bom);
    }

    #[test]
    fn numeric_dot_and_tuple_index_rules_are_contextual_but_semantic_free() {
        let lexed = lex_text("0..3 0..=3 0...tail 1.to_str 1.0.to_str pair . /*x*/ 0.1");

        assert!(lexed.is_success());
        assert_eq!(
            significant(&lexed),
            [
                (TokenKind::Integer, "0"),
                (TokenKind::Range, ".."),
                (TokenKind::Integer, "3"),
                (TokenKind::Integer, "0"),
                (TokenKind::RangeInclusive, "..="),
                (TokenKind::Integer, "3"),
                (TokenKind::Integer, "0"),
                (TokenKind::AxisPackNamed, "...tail"),
                (TokenKind::Integer, "1"),
                (TokenKind::Dot, "."),
                (TokenKind::Identifier, "to_str"),
                (TokenKind::Float, "1.0"),
                (TokenKind::Dot, "."),
                (TokenKind::Identifier, "to_str"),
                (TokenKind::Identifier, "pair"),
                (TokenKind::Dot, "."),
                (TokenKind::TupleIndex, "0"),
                (TokenKind::Dot, "."),
                (TokenKind::TupleIndex, "1"),
            ]
            .map(|(kind, text)| (kind, text.to_owned()))
        );
    }

    #[test]
    fn decimal_components_share_strict_interior_separator_rules() {
        let lexed = lex_text("12 1_000 1_000.25_00 1e1_0 1.25E-1_0 pair.1_0 #1_0 _1 1._0");

        assert!(lexed.is_success());
        assert_eq!(
            significant(&lexed),
            [
                (TokenKind::Integer, "12"),
                (TokenKind::Integer, "1_000"),
                (TokenKind::Float, "1_000.25_00"),
                (TokenKind::Float, "1e1_0"),
                (TokenKind::Float, "1.25E-1_0"),
                (TokenKind::Identifier, "pair"),
                (TokenKind::Dot, "."),
                (TokenKind::TupleIndex, "1_0"),
                (TokenKind::AxisIndex, "#1_0"),
                (TokenKind::Identifier, "_1"),
                (TokenKind::Integer, "1"),
                (TokenKind::Dot, "."),
                (TokenKind::Identifier, "_0"),
            ]
            .map(|(kind, text)| (kind, text.to_owned()))
        );
    }

    #[test]
    fn malformed_decimal_components_keep_lexical_diagnostic_ownership() {
        for source in [
            "1_", "1__0", "1_.0", "1.2_", "1.2__0", "1e_2", "1e+_2", "1e2_", "#1_", "#1__0",
        ] {
            let lexed = lex_text(source);
            assert_eq!(
                lexed.diagnostics()[0].kind(),
                SyntaxDiagnosticKind::LexInvalidNumber,
                "source: {source}"
            );
        }

        for source in ["#", "#_1"] {
            let lexed = lex_text(source);
            assert_eq!(
                lexed.diagnostics()[0].kind(),
                SyntaxDiagnosticKind::LexInvalidChar,
                "source: {source}"
            );
        }
    }

    #[test]
    fn unicode_identifier_scalars_and_other_unicode_have_distinct_diagnostics() {
        assert_eq!(unicode_ident::UNICODE_VERSION, (17, 0, 0));

        for source in ["변수", "\u{0301}", "'변수"] {
            let lexed = lex_text(source);
            assert_eq!(
                lexed.diagnostics()[0].kind(),
                SyntaxDiagnosticKind::LexInvalidIdentifierChar,
                "source: {source:?}"
            );
        }

        for source in ["😀", "\u{0085}", "\u{2028}", "\u{2029}", "\u{000b}"] {
            let lexed = lex_text(source);
            assert_eq!(
                lexed.diagnostics()[0].kind(),
                SyntaxDiagnosticKind::LexInvalidChar,
                "source: {source:?}"
            );
        }

        let axis = lex_text("'변수");
        assert_eq!(axis.diagnostics()[0].span().range().start().get(), 1);
        assert_eq!(axis.diagnostics()[0].span().range().end().get(), 4);
    }

    #[test]
    fn physical_newlines_and_horizontal_whitespace_keep_exact_token_bytes() {
        let lexed = lex_text(" \t\u{000c}a\r\nb\rc\nd");
        let newlines = lexed
            .tokens()
            .iter()
            .filter(|token| token.kind() == TokenKind::Newline)
            .map(|token| lexed.text(*token).expect("token must belong to source"))
            .collect::<Vec<_>>();

        assert!(lexed.is_success());
        assert_eq!(newlines, ["\r\n", "\r", "\n"]);
        assert_eq!(lexed.tokens()[0].kind(), TokenKind::Whitespace);
        assert_eq!(lexed.text(lexed.tokens()[0]), Some(" \t\u{000c}"));
    }

    #[test]
    fn block_comment_newlines_are_physical_tokens_only_when_valid() {
        let valid = lex_text("a/* first\r\nsecond\rthird\n*/b");
        let newlines = valid
            .tokens()
            .iter()
            .filter(|token| token.kind() == TokenKind::Newline)
            .map(|token| {
                valid
                    .text(*token)
                    .expect("token should belong to its lexical buffer")
            })
            .collect::<Vec<_>>();
        assert_eq!(newlines, ["\r\n", "\r", "\n"]);

        let line_comment = lex_text("a// comment\rb");
        assert!(line_comment.is_success());
        assert!(line_comment.tokens().iter().any(|token| {
            token.kind() == TokenKind::Newline && line_comment.text(*token) == Some("\r")
        }));

        let nested = lex_text("a/* first\n/* nested */b");
        assert_eq!(
            nested.diagnostics()[0].kind(),
            SyntaxDiagnosticKind::LexNestedBlockComment
        );
        assert!(
            !nested
                .tokens()
                .iter()
                .any(|token| token.kind() == TokenKind::Newline)
        );
        assert_eq!(
            nested
                .tokens()
                .iter()
                .map(|token| {
                    nested
                        .text(*token)
                        .expect("token should belong to its lexical buffer")
                })
                .collect::<String>(),
            "a/* first\n/* nested */b"
        );
    }

    #[test]
    fn strings_validate_required_escape_and_unicode_scalar_rules() {
        let valid = lex_text(r#""\\\"\n\r\t\u{10ffff}" f"value={name}""#);
        assert!(valid.is_success());
        assert_eq!(
            significant(&valid),
            [
                (TokenKind::String, r#""\\\"\n\r\t\u{10ffff}""#.to_owned()),
                (TokenKind::FormatString, r#"f"value={name}""#.to_owned()),
            ]
        );

        for source in [r#""\q""#, r#""\u{}""#, r#""\u{d800}""#, r#""\u{110000}""#] {
            let invalid = lex_text(source);
            assert_eq!(
                invalid.diagnostics()[0].kind(),
                SyntaxDiagnosticKind::LexInvalidEscape,
                "source: {source}"
            );
        }
    }

    #[test]
    fn reserved_and_contextual_words_remain_distinct() {
        let lexed = lex_text("fn main! view self closed Self Fn own move drop_own _ _x!");

        assert_eq!(
            significant(&lexed)
                .into_iter()
                .map(|(kind, _)| kind)
                .collect::<Vec<_>>(),
            [
                TokenKind::FnKw,
                TokenKind::BangIdentifier,
                TokenKind::Identifier,
                TokenKind::Identifier,
                TokenKind::Identifier,
                TokenKind::Identifier,
                TokenKind::Identifier,
                TokenKind::OwnKw,
                TokenKind::MoveKw,
                TokenKind::DropOwnKw,
                TokenKind::Underscore,
                TokenKind::BangIdentifier,
            ]
        );
    }

    #[test]
    fn lexical_failure_retains_error_and_raw_suffix() {
        let lexed = lex_text("let x = 1.2.3\nlet y = 4");

        assert_eq!(lexed.diagnostics().len(), 1);
        assert_eq!(
            lexed.diagnostics()[0].kind(),
            SyntaxDiagnosticKind::LexInvalidNumber
        );
        assert_eq!(
            lexed.diagnostics()[0].span().range().start(),
            ByteOffset::new(8)
        );
        assert_eq!(
            lexed
                .tokens()
                .iter()
                .map(|token| {
                    lexed
                        .text(*token)
                        .expect("token should belong to its lexical buffer")
                })
                .collect::<String>(),
            "let x = 1.2.3\nlet y = 4"
        );
        assert_eq!(lexed.tokens().last().unwrap().kind(), TokenKind::Raw);
    }

    #[test]
    fn alias_delimiter_requires_a_same_line_identifier_binder() {
        for source in ["whole @ Some(value)", "whole@Some(value)"] {
            let lexed = lex_text(source);
            assert!(lexed.is_success(), "source: {source}");
            assert!(
                significant(&lexed)
                    .iter()
                    .any(|(kind, text)| *kind == TokenKind::At && text == "@"),
                "source: {source}"
            );
        }

        for source in ["@x", "whole\n@Some(value)"] {
            let lexed = lex_text(source);
            assert_eq!(
                lexed.diagnostics()[0].kind(),
                SyntaxDiagnosticKind::LexInvalidChar,
                "source: {source}"
            );
        }
    }

    #[test]
    fn token_text_rejects_a_token_from_another_source() {
        let left_source = SourceFile::new(FileId::new(1), b"left".to_vec());
        let right_source = SourceFile::new(FileId::new(2), b"right".to_vec());
        let left = lex(left_source.validate().expect("left source should validate"));
        let right = lex(right_source
            .validate()
            .expect("right source should validate"));

        assert_eq!(left.text(right.tokens()[0]), None);
    }

    #[test]
    fn axis_and_operator_maximal_munch_is_complete() {
        let lexed = lex_text(
            "'batch #12 #[ -> => :: ... ..= .. /. == != <= >= && || ? = < > + - * / % ! & | : , . ( ) { } [ ]",
        );

        assert!(lexed.is_success());
        assert_eq!(
            significant(&lexed)
                .into_iter()
                .map(|(kind, _)| kind)
                .collect::<Vec<_>>(),
            [
                TokenKind::AxisName,
                TokenKind::AxisIndex,
                TokenKind::HashLeftBracket,
                TokenKind::Arrow,
                TokenKind::FatArrow,
                TokenKind::ColonColon,
                TokenKind::AxisPackAnonymous,
                TokenKind::RangeInclusive,
                TokenKind::Range,
                TokenKind::FractionalDivide,
                TokenKind::EqualEqual,
                TokenKind::NotEqual,
                TokenKind::LessEqual,
                TokenKind::GreaterEqual,
                TokenKind::LogicalAnd,
                TokenKind::LogicalOr,
                TokenKind::Question,
                TokenKind::Equal,
                TokenKind::Less,
                TokenKind::Greater,
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Bang,
                TokenKind::Ampersand,
                TokenKind::Pipe,
                TokenKind::Colon,
                TokenKind::Comma,
                TokenKind::Dot,
                TokenKind::LeftParen,
                TokenKind::RightParen,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
                TokenKind::LeftBracket,
                TokenKind::RightBracket,
            ]
        );
    }
}

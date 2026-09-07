use eglur_source::{ByteRange, FileSpan};

use crate::TokenKind;

/// One non-empty physical source partition produced by the lexer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token {
    kind: TokenKind,
    span: FileSpan,
}

impl Token {
    /// Creates a physical token over an already checked range.
    #[must_use]
    pub(crate) const fn new(kind: TokenKind, span: FileSpan) -> Self {
        Self { kind, span }
    }

    /// Returns the physical token kind.
    #[must_use]
    pub const fn kind(self) -> TokenKind {
        self.kind
    }

    /// Returns the exact source identity and byte range.
    #[must_use]
    pub const fn span(self) -> FileSpan {
        self.span
    }

    /// Returns the exact half-open byte range.
    #[must_use]
    pub const fn range(self) -> ByteRange {
        self.span.range()
    }
}

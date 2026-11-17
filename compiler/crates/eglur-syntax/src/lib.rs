//! Production boundary for lossless lexing, parsing, recovery, and syntax access.

#![forbid(unsafe_code)]

pub mod ast;

mod diagnostic;
mod event;
mod frontend;
mod kind;
mod lexer;
mod parser;
mod syntax_kind;
mod token;
mod tree;

#[cfg(test)]
mod corpus_tests;
#[cfg(test)]
mod measurement;
#[cfg(test)]
mod test_corpus;

pub use diagnostic::{
    ParseResource, ParseResourceLimit, SyntaxDiagnostic, SyntaxDiagnosticKind, SyntaxPhase,
};
pub use frontend::{
    FrontendCountUnit, FrontendOutcome, FrontendPhase, FrontendPhaseObservation, ObservedFrontend,
    ObservedParse, Parse, parse, parse_observed, parse_validated, parse_validated_observed,
};
pub use kind::TokenKind;
pub use lexer::{Lexed, is_identifier_spelling, lex};
pub use syntax_kind::SyntaxKind;
pub use token::Token;
pub use tree::{SyntaxElement, SyntaxNode, SyntaxToken};

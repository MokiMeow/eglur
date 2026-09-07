use std::time::{Duration, Instant};

use eglur_source::{FileId, SourceFile, SourceValidationError, ValidatedSource};

use crate::ast::AstNode;
use crate::parser::{ParseEntryPoint, parse_lexed};
use crate::tree::{GreenTree, build_tree, error_tree_events};
use crate::{Lexed, SyntaxDiagnostic, SyntaxNode, ast, lex};

/// Complete source-front-end outcome before semantic analysis.
#[derive(Clone, Debug)]
pub enum FrontendOutcome {
    Parsed(Parse),
    Rejected(SourceValidationError),
}

/// Stable front-end phase names shared with compiler-session observations.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FrontendPhase {
    Lexer,
    ParserEvents,
    TreeBuild,
}

impl FrontendPhase {
    /// Returns the phase spelling used by front-end evidence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexer => "lexer",
            Self::ParserEvents => "parser_events",
            Self::TreeBuild => "tree_build",
        }
    }

    /// Returns the unit used by this phase's input count.
    #[must_use]
    pub const fn input_unit(self) -> FrontendCountUnit {
        match self {
            Self::Lexer => FrontendCountUnit::Bytes,
            Self::ParserEvents => FrontendCountUnit::Tokens,
            Self::TreeBuild => FrontendCountUnit::Events,
        }
    }

    /// Returns the unit used by this phase's output count.
    #[must_use]
    pub const fn output_unit(self) -> FrontendCountUnit {
        match self {
            Self::Lexer => FrontendCountUnit::Tokens,
            Self::ParserEvents => FrontendCountUnit::Events,
            Self::TreeBuild => FrontendCountUnit::Trees,
        }
    }
}

/// Count unit for one observed front-end phase boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendCountUnit {
    Bytes,
    Tokens,
    Events,
    Trees,
}

/// One opt-in observation of an exact front-end phase invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendPhaseObservation {
    phase: FrontendPhase,
    input_count: usize,
    output_count: usize,
    elapsed: Duration,
}

impl FrontendPhaseObservation {
    /// Returns the observed phase.
    #[must_use]
    pub const fn phase(self) -> FrontendPhase {
        self.phase
    }

    /// Returns the phase-defined input count.
    #[must_use]
    pub const fn input_count(self) -> usize {
        self.input_count
    }

    /// Returns the phase-defined output count.
    #[must_use]
    pub const fn output_count(self) -> usize {
        self.output_count
    }

    /// Returns elapsed wall-clock time for this phase invocation.
    #[must_use]
    pub const fn elapsed(self) -> Duration {
        self.elapsed
    }
}

/// Front-end result paired with opt-in phase observations.
#[derive(Clone, Debug)]
pub struct ObservedFrontend {
    outcome: FrontendOutcome,
    observations: Box<[FrontendPhaseObservation]>,
}

/// Validated-source parse paired with opt-in phase observations.
#[derive(Clone, Debug)]
pub struct ObservedParse {
    parse: Parse,
    observations: Box<[FrontendPhaseObservation]>,
}

impl ObservedParse {
    /// Returns the parser product.
    #[must_use]
    pub const fn parse(&self) -> &Parse {
        &self.parse
    }

    /// Returns observations in execution order.
    #[must_use]
    pub fn observations(&self) -> &[FrontendPhaseObservation] {
        &self.observations
    }

    /// Separates the parse and observations without copying either.
    #[must_use]
    pub fn into_parts(self) -> (Parse, Box<[FrontendPhaseObservation]>) {
        (self.parse, self.observations)
    }
}

impl ObservedFrontend {
    /// Returns the front-end result.
    #[must_use]
    pub const fn outcome(&self) -> &FrontendOutcome {
        &self.outcome
    }

    /// Returns observations in execution order.
    #[must_use]
    pub fn observations(&self) -> &[FrontendPhaseObservation] {
        &self.observations
    }

    /// Separates the result from its observations without copying either.
    #[must_use]
    pub fn into_parts(self) -> (FrontendOutcome, Box<[FrontendPhaseObservation]>) {
        (self.outcome, self.observations)
    }
}

/// Immutable lossless syntax tree and syntax diagnostics for one source file.
#[derive(Clone, Debug)]
pub struct Parse {
    file: FileId,
    tree: GreenTree,
    diagnostics: Box<[SyntaxDiagnostic]>,
}

impl Parse {
    /// Returns the source identity represented by this parse.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Returns all retained lexical or parser diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[SyntaxDiagnostic] {
        &self.diagnostics
    }

    /// Returns the opaque lossless root node.
    #[must_use]
    pub fn syntax(&self) -> SyntaxNode {
        self.tree.root(self.file)
    }

    /// Returns the typed source-file facade.
    #[must_use]
    pub fn root(&self) -> ast::SourceFile {
        ast::SourceFile::cast(self.syntax()).expect("parse root kind is stable")
    }

    /// Reports whether source validation, lexing, and parsing all succeeded.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Parses one complete source file without running semantic compiler phases.
#[must_use]
pub fn parse(file: &SourceFile) -> FrontendOutcome {
    parse_with_entry(file, ParseEntryPoint::CompilationUnit)
}

/// Parses one source while recording exact lexer, parser-event, and tree phases.
///
/// Timing is opt-in: ordinary [`parse`] performs the same operations without
/// reading the wall clock.
#[must_use]
pub fn parse_observed(file: &SourceFile) -> ObservedFrontend {
    let mut observations = Vec::new();
    let outcome = parse_impl(
        file,
        ParseEntryPoint::CompilationUnit,
        Some(&mut observations),
    );
    ObservedFrontend {
        outcome,
        observations: observations.into_boxed_slice(),
    }
}

/// Parses source whose size and UTF-8 validity were already established.
#[must_use]
pub fn parse_validated(source: ValidatedSource<'_>) -> Parse {
    finish_validated(source, ParseEntryPoint::CompilationUnit, None)
}

/// Parses validated source while recording exact front-end phase observations.
#[must_use]
pub fn parse_validated_observed(source: ValidatedSource<'_>) -> ObservedParse {
    let mut observations = Vec::new();
    let parse = finish_validated(
        source,
        ParseEntryPoint::CompilationUnit,
        Some(&mut observations),
    );
    ObservedParse {
        parse,
        observations: observations.into_boxed_slice(),
    }
}

pub(crate) fn parse_with_entry(file: &SourceFile, entry: ParseEntryPoint) -> FrontendOutcome {
    parse_impl(file, entry, None)
}

fn parse_impl(
    file: &SourceFile,
    entry: ParseEntryPoint,
    observations: Option<&mut Vec<FrontendPhaseObservation>>,
) -> FrontendOutcome {
    let source = match file.validate() {
        Ok(source) => source,
        Err(error) => return FrontendOutcome::Rejected(error),
    };
    FrontendOutcome::Parsed(finish_validated(source, entry, observations))
}

fn finish_validated(
    source: ValidatedSource<'_>,
    entry: ParseEntryPoint,
    mut observations: Option<&mut Vec<FrontendPhaseObservation>>,
) -> Parse {
    let lexed = measure_phase(
        observations.as_deref_mut(),
        FrontendPhase::Lexer,
        source.bytes().len(),
        || lex(source),
        |lexed| lexed.tokens().len(),
    );
    finish_lexed(&lexed, entry, observations)
}

fn finish_lexed(
    lexed: &Lexed<'_>,
    entry: ParseEntryPoint,
    mut observations: Option<&mut Vec<FrontendPhaseObservation>>,
) -> Parse {
    let (events, diagnostics) = if lexed.is_success() {
        measure_phase(
            observations.as_deref_mut(),
            FrontendPhase::ParserEvents,
            lexed.tokens().len(),
            || parse_lexed(lexed, entry),
            |(events, _)| events.len(),
        )
    } else {
        (error_tree_events(lexed), lexed.diagnostics().into())
    };
    let event_count = events.len();
    let tree = measure_phase(
        observations,
        FrontendPhase::TreeBuild,
        event_count,
        || build_tree(lexed, events),
        |_| 1,
    );
    Parse {
        file: lexed.source().id(),
        tree,
        diagnostics,
    }
}

fn measure_phase<T>(
    observations: Option<&mut Vec<FrontendPhaseObservation>>,
    phase: FrontendPhase,
    input_count: usize,
    operation: impl FnOnce() -> T,
    output_count: impl FnOnce(&T) -> usize,
) -> T {
    let Some(observations) = observations else {
        return operation();
    };

    let started = Instant::now();
    let output = operation();
    observations.push(FrontendPhaseObservation {
        phase,
        input_count,
        output_count: output_count(&output),
        elapsed: started.elapsed(),
    });
    output
}

#[cfg(test)]
mod tests {
    use eglur_source::{FileId, SourceFile, SourceValidationError};

    use super::{FrontendCountUnit, FrontendOutcome, FrontendPhase, parse, parse_observed};

    #[test]
    fn observed_parse_preserves_the_frontend_result_and_phase_boundaries() {
        let source = SourceFile::new(FileId::new(7), b"fn main() -> i64 = 1\n".to_vec());
        let ordinary = parse(&source);
        let observed = parse_observed(&source);

        let FrontendOutcome::Parsed(ordinary) = ordinary else {
            panic!("valid source should parse");
        };
        let FrontendOutcome::Parsed(observed_parse) = observed.outcome() else {
            panic!("observed valid source should parse");
        };
        assert_eq!(ordinary.file(), observed_parse.file());
        assert_eq!(ordinary.diagnostics(), observed_parse.diagnostics());
        assert_eq!(ordinary.syntax().text(), observed_parse.syntax().text());

        let phases = observed
            .observations()
            .iter()
            .map(|observation| observation.phase())
            .collect::<Vec<_>>();
        assert_eq!(
            phases,
            [
                FrontendPhase::Lexer,
                FrontendPhase::ParserEvents,
                FrontendPhase::TreeBuild,
            ]
        );
        assert_eq!(FrontendPhase::Lexer.input_unit(), FrontendCountUnit::Bytes);
        assert_eq!(
            FrontendPhase::TreeBuild.output_unit(),
            FrontendCountUnit::Trees
        );
        assert_eq!(observed.observations()[0].input_count(), source.len());
        assert_eq!(observed.observations()[2].output_count(), 1);
    }

    #[test]
    fn rejected_source_has_no_frontend_phase_observations() {
        let source = SourceFile::new(FileId::new(3), b"ok\xff".to_vec());
        let observed = parse_observed(&source);

        assert!(matches!(
            observed.outcome(),
            FrontendOutcome::Rejected(SourceValidationError::InvalidUtf8(_))
        ));
        assert!(observed.observations().is_empty());
    }

    #[test]
    fn parser_diagnostics_retain_their_complete_recovery_region() {
        let text = "pub #[derive(LocalTrait)]\nstruct S {}\n\ntrait LocalTrait {}\n";
        let source = SourceFile::new(FileId::new(5), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parse) = parse(&source) else {
            panic!("UTF-8 source should reach the parser");
        };
        let diagnostic = parse
            .diagnostics()
            .first()
            .copied()
            .expect("misordered attribute should be diagnosed");

        assert_eq!(diagnostic.span().range().start().get(), 4);
        assert_eq!(diagnostic.recovery_span().range().start().get(), 0);
        assert_eq!(
            diagnostic.recovery_span().range().end().as_usize(),
            text.find("struct")
                .expect("fixture should contain the next valid item")
        );
    }
}

use eglur_source::FileSpan;

use crate::{OriginId, SyntaxCauseId};

/// A source-level rewrite that produced one semantic occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DesugaringKind {
    TrailingArgument,
    PostfixThen,
    PostfixApply,
    HolePartialApplication,
    ReduceByOrdinaryCandidate,
    ReduceBySingleReducerCandidate,
    ReduceByOrderedPhasesCandidate,
}

/// Exact written provenance for one HIR occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceOrigin {
    Written(FileSpan),
    Derived {
        source: FileSpan,
        kind: DesugaringKind,
    },
}

impl SourceOrigin {
    /// Returns the exact user-written source range represented by this origin.
    #[must_use]
    pub const fn source(self) -> FileSpan {
        match self {
            Self::Written(source) | Self::Derived { source, .. } => source,
        }
    }

    /// Returns the rewrite kind for a derived occurrence.
    #[must_use]
    pub const fn desugaring(self) -> Option<DesugaringKind> {
        match self {
            Self::Written(_) => None,
            Self::Derived { kind, .. } => Some(kind),
        }
    }
}

/// Structural category for syntax that cannot form a valid HIR occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InvalidCategory {
    ReportedSyntax,
    Item,
    Name,
    Literal,
    Attribute,
    Type,
    Statement,
    Expression,
    Pattern,
    TensorAxis,
}

/// One source-bound structural invalid occurrence, without semantic poison.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InvalidOccurrence {
    category: InvalidCategory,
    origin: OriginId,
    syntax_causes: Box<[SyntaxCauseId]>,
}

impl InvalidOccurrence {
    pub(crate) fn new(category: InvalidCategory, origin: OriginId) -> Self {
        Self {
            category,
            origin,
            syntax_causes: Box::new([]),
        }
    }

    pub(crate) fn with_syntax_causes(
        category: InvalidCategory,
        origin: OriginId,
        syntax_causes: Box<[SyntaxCauseId]>,
    ) -> Self {
        Self {
            category,
            origin,
            syntax_causes,
        }
    }

    pub(crate) fn set_syntax_causes(&mut self, syntax_causes: Box<[SyntaxCauseId]>) {
        self.syntax_causes = syntax_causes;
    }

    #[must_use]
    pub const fn category(&self) -> InvalidCategory {
        self.category
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Returns the parser causes that authorized this recovery occurrence.
    #[must_use]
    pub fn syntax_causes(&self) -> &[SyntaxCauseId] {
        &self.syntax_causes
    }
}

use std::num::NonZeroUsize;

use crate::{Diagnostic, Severity};

/// Positive deterministic cap on retained root errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ErrorLimit(NonZeroUsize);

impl ErrorLimit {
    /// Creates a positive error limit.
    #[must_use]
    pub const fn new(limit: usize) -> Option<Self> {
        match NonZeroUsize::new(limit) {
            Some(limit) => Some(Self(limit)),
            None => None,
        }
    }

    /// Returns the configured maximum number of retained root errors.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FinalizedDiagnostics {
    diagnostics: Box<[Diagnostic]>,
    omitted_error_count: usize,
}

/// Finalized diagnostics proven to contain no error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedDiagnostics(FinalizedDiagnostics);

impl AcceptedDiagnostics {
    /// Returns warning and note diagnostics in canonical order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.0.diagnostics
    }
}

/// Finalized diagnostics proven to retain at least one root error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectedDiagnostics(FinalizedDiagnostics);

impl RejectedDiagnostics {
    /// Returns all retained diagnostics in canonical order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.0.diagnostics
    }

    /// Returns the first error under canonical ordering.
    #[must_use]
    pub fn first_error(&self) -> &Diagnostic {
        self.0
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity() == Severity::Error)
            .expect("rejected diagnostics always retain an error")
    }

    /// Returns the number of root errors omitted by the configured limit.
    #[must_use]
    pub const fn omitted_error_count(&self) -> usize {
        self.0.omitted_error_count
    }
}

/// Immutable ordered presentation source after root retention and truncation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticSnapshot {
    Accepted(AcceptedDiagnostics),
    Rejected(RejectedDiagnostics),
}

impl DiagnosticSnapshot {
    pub(crate) fn finalize(
        mut errors: Vec<Diagnostic>,
        observations: Vec<Diagnostic>,
        limit: Option<ErrorLimit>,
    ) -> Self {
        errors.sort();
        let established_error_count = errors.len();
        if let Some(limit) = limit {
            errors.truncate(limit.get());
        }
        let omitted_error_count = established_error_count - errors.len();
        let rejected = !errors.is_empty();

        let mut diagnostics = errors;
        diagnostics.extend(observations);
        diagnostics.sort();
        let finalized = FinalizedDiagnostics {
            diagnostics: diagnostics.into_boxed_slice(),
            omitted_error_count,
        };

        if rejected {
            Self::Rejected(RejectedDiagnostics(finalized))
        } else {
            debug_assert_eq!(established_error_count, 0);
            debug_assert_eq!(omitted_error_count, 0);
            Self::Accepted(AcceptedDiagnostics(finalized))
        }
    }

    /// Returns every retained diagnostic in canonical order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Accepted(diagnostics) => diagnostics.diagnostics(),
            Self::Rejected(diagnostics) => diagnostics.diagnostics(),
        }
    }

    /// Projects the first ordered error when compilation was rejected.
    #[must_use]
    pub fn first_error(&self) -> Option<&Diagnostic> {
        match self {
            Self::Accepted(_) => None,
            Self::Rejected(diagnostics) => Some(diagnostics.first_error()),
        }
    }

    /// Returns the number of established roots omitted by the error limit.
    #[must_use]
    pub const fn omitted_error_count(&self) -> usize {
        match self {
            Self::Accepted(_) => 0,
            Self::Rejected(diagnostics) => diagnostics.omitted_error_count(),
        }
    }
}

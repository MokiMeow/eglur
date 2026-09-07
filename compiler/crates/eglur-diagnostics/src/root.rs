use std::fmt;

use eglur_source::ByteRange;

use crate::{Diagnostic, DiagnosticCode, PrimarySpan, Severity, SourceLabel};

/// Stable caller-owned identity for one source-unavailable failing boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BoundaryRootKey(Box<str>);

impl BoundaryRootKey {
    /// Creates a non-empty boundary identity used only within compilation.
    #[must_use]
    pub fn new(key: impl Into<Box<str>>) -> Option<Self> {
        let key = key.into();
        (!key.is_empty()).then_some(Self(key))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RootIdentity {
    Source {
        code: DiagnosticCode,
        source: SourceLabel,
        bytes: ByteRange,
    },
    Boundary {
        code: DiagnosticCode,
        key: BoundaryRootKey,
    },
}

/// Invalid relationship between an error record and its claimed root shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticRootError {
    NonErrorSeverity(Severity),
    SourcePrimaryRequired,
    UnavailablePrimaryRequired,
}

impl fmt::Display for DiagnosticRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonErrorSeverity(severity) => {
                write!(
                    formatter,
                    "diagnostic root must be an error, not {severity}"
                )
            }
            Self::SourcePrimaryRequired => {
                formatter.write_str("source root requires a source-bound primary span")
            }
            Self::UnavailablePrimaryRequired => {
                formatter.write_str("boundary root requires an unavailable primary span")
            }
        }
    }
}

impl std::error::Error for DiagnosticRootError {}

/// One phase-established independent diagnostic root claim.
pub struct DiagnosticRoot {
    identity: RootIdentity,
    diagnostic: Diagnostic,
}

impl DiagnosticRoot {
    /// Derives identity from the stable code and canonical source blame.
    pub fn source(diagnostic: Diagnostic) -> Result<Self, DiagnosticRootError> {
        require_error(&diagnostic)?;
        let Some(primary) = diagnostic.primary_span().as_source() else {
            return Err(DiagnosticRootError::SourcePrimaryRequired);
        };
        let identity = RootIdentity::Source {
            code: diagnostic.code().clone(),
            source: primary.source().clone(),
            bytes: primary.file_span().range(),
        };
        Ok(Self {
            identity,
            diagnostic,
        })
    }

    /// Identifies one source-unavailable boundary failure explicitly.
    pub fn boundary(
        key: BoundaryRootKey,
        diagnostic: Diagnostic,
    ) -> Result<Self, DiagnosticRootError> {
        require_error(&diagnostic)?;
        if !matches!(diagnostic.primary_span(), PrimarySpan::Unavailable) {
            return Err(DiagnosticRootError::UnavailablePrimaryRequired);
        }
        let identity = RootIdentity::Boundary {
            code: diagnostic.code().clone(),
            key,
        };
        Ok(Self {
            identity,
            diagnostic,
        })
    }

    pub(crate) fn into_parts(self) -> (RootIdentity, Diagnostic) {
        (self.identity, self.diagnostic)
    }
}

impl fmt::Debug for DiagnosticRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiagnosticRoot")
            .field("diagnostic", &self.diagnostic)
            .finish_non_exhaustive()
    }
}

fn require_error(diagnostic: &Diagnostic) -> Result<(), DiagnosticRootError> {
    if diagnostic.severity() == Severity::Error {
        Ok(())
    } else {
        Err(DiagnosticRootError::NonErrorSeverity(diagnostic.severity()))
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eglur_diagnostics::{DiagnosticStoreError, Poison, SourceSpan};
use eglur_hir::{HirFile, OriginId};
use eglur_source::FileId;

use crate::ModuleAddress;

/// Normalized semantic-phase input for one driver-owned compilation unit.
pub struct SemanticSource<'hir> {
    file: FileId,
    module: ModuleAddress,
    hir: Option<&'hir HirFile>,
    origin_spans: BTreeMap<OriginId, SourceSpan>,
    invalid_poisons: BTreeMap<OriginId, Poison>,
    source_poison: Option<Poison>,
}

impl<'hir> SemanticSource<'hir> {
    pub fn new(
        file: FileId,
        module: ModuleAddress,
        hir: Option<&'hir HirFile>,
        origin_spans: impl IntoIterator<Item = (OriginId, SourceSpan)>,
        invalid_poisons: impl IntoIterator<Item = (OriginId, Poison)>,
        source_poison: Option<Poison>,
    ) -> Result<Self, SourceInputError> {
        if hir.is_some_and(|hir| hir.file() != file) {
            return Err(SourceInputError::HirFileMismatch);
        }
        let mut retained_spans = BTreeMap::new();
        for (origin, span) in origin_spans {
            if retained_spans.insert(origin, span).is_some() {
                return Err(SourceInputError::DuplicateOriginSpan);
            }
        }
        if retained_spans
            .values()
            .any(|span| span.file_span().file() != file)
        {
            return Err(SourceInputError::SpanFileMismatch);
        }
        if let Some(hir) = hir {
            let expected_origins = hir
                .origins()
                .map(|(origin, _)| origin)
                .collect::<BTreeSet<_>>();
            if retained_spans.keys().copied().collect::<BTreeSet<_>>() != expected_origins {
                return Err(SourceInputError::OriginSpanCoverage);
            }
        } else if !retained_spans.is_empty() {
            return Err(SourceInputError::OriginWithoutHir);
        }

        let mut merged_invalid = BTreeMap::<OriginId, Poison>::new();
        for (origin, poison) in invalid_poisons {
            if let Some(existing) = merged_invalid.get(&origin) {
                merged_invalid.insert(origin, existing.merge(&poison)?);
            } else {
                merged_invalid.insert(origin, poison);
            }
        }
        let expected_invalid = hir
            .map(|hir| {
                hir.invalid_occurrences()
                    .iter()
                    .map(|occurrence| occurrence.origin())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        if merged_invalid.keys().copied().collect::<BTreeSet<_>>() != expected_invalid {
            return Err(SourceInputError::InvalidPoisonCoverage);
        }
        Ok(Self {
            file,
            module,
            hir,
            origin_spans: retained_spans,
            invalid_poisons: merged_invalid,
            source_poison,
        })
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        &self.module
    }

    #[must_use]
    pub const fn hir(&self) -> Option<&'hir HirFile> {
        self.hir
    }

    #[must_use]
    pub fn origin_span(&self, origin: OriginId) -> Option<&SourceSpan> {
        self.origin_spans.get(&origin)
    }

    #[must_use]
    pub fn invalid_poison(&self, origin: OriginId) -> Option<&Poison> {
        self.invalid_poisons.get(&origin)
    }

    #[must_use]
    pub const fn source_poison(&self) -> Option<&Poison> {
        self.source_poison.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceInputError {
    HirFileMismatch,
    SpanFileMismatch,
    DuplicateOriginSpan,
    OriginSpanCoverage,
    OriginWithoutHir,
    InvalidPoisonCoverage,
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for SourceInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HirFileMismatch => {
                formatter.write_str("semantic source HIR belongs to another file")
            }
            Self::SpanFileMismatch => {
                formatter.write_str("semantic source span belongs to another file")
            }
            Self::DuplicateOriginSpan => {
                formatter.write_str("semantic source repeated a HIR origin span")
            }
            Self::OriginSpanCoverage => {
                formatter.write_str("semantic source origin spans do not exactly cover its HIR")
            }
            Self::OriginWithoutHir => {
                formatter.write_str("semantic source has origin spans without HIR")
            }
            Self::InvalidPoisonCoverage => formatter
                .write_str("semantic source invalid-poison identities do not match recovered HIR"),
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SourceInputError {}

impl From<DiagnosticStoreError> for SourceInputError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}

use std::fmt;

use eglur_diagnostics::{DiagnosticRootError, DiagnosticStoreError};
use eglur_hir::OriginId;
use eglur_source::FileId;

use crate::diagnostic::SemanticDiagnosticError;
use crate::type_namespace::TypeNameLookupError;
use crate::type_rendering::DiagnosticTypeRenderingError;
use crate::{
    CanonicalTarget, DefId, DuplicateFact, ExternalDefId, InhabitanceError, LanguageItem,
    PrimitiveType, TypeStoreError,
};

use super::TrustedSignatureError;

/// Signature-phase infrastructure failure; source errors are recorded as diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignatureResolutionError {
    MissingAnalysisStop,
    UnexpectedUnmeteredInterruption,
    AnalysisRootClaim(crate::AnalysisRootClaimError),
    DuplicateSource(FileId),
    MissingSource(FileId),
    MissingHir(FileId),
    SourceModuleMismatch(FileId),
    MissingExternalNames,
    MissingTrustedInhabitanceSignature(ExternalDefId),
    TrustedInhabitanceOnNonNominal(ExternalDefId),
    MissingLanguageItemSignature {
        item: LanguageItem,
        declaration: ExternalDefId,
    },
    InvalidLanguageItemSignature {
        item: LanguageItem,
        declaration: ExternalDefId,
    },
    MissingTrustedSignature(CanonicalTarget),
    UnexpectedPrimitiveTarget {
        primitive: PrimitiveType,
        expected: &'static str,
    },
    UnexpectedIntrinsicTarget {
        intrinsic: crate::IntrinsicNominal,
        expected: &'static str,
    },
    InvalidSourceHeadState(DefId),
    MissingDeclarationItem(DefId),
    WrongDeclarationItem(DefId),
    MissingGenericDeclaration(DefId),
    MissingOriginSpan {
        file: FileId,
        origin: OriginId,
    },
    MissingInvalidPoison {
        file: FileId,
        origin: OriginId,
    },
    InvalidImportFact(crate::ImportId),
    DiagnosticTypeRendering(Box<str>),
    TypeStore(TypeStoreError),
    DuplicateFact(DuplicateFact),
    DiagnosticRoot(DiagnosticRootError),
    DiagnosticStore(DiagnosticStoreError),
    Trusted(TrustedSignatureError),
    Inhabitance(InhabitanceError),
    Identity(crate::IdentityOverflow),
}

impl fmt::Display for SignatureResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAnalysisStop => {
                formatter.write_str("signature interruption has no accounting stop")
            }
            Self::UnexpectedUnmeteredInterruption => {
                formatter.write_str("unmetered signature analysis was interrupted")
            }
            Self::AnalysisRootClaim(error) => write!(
                formatter,
                "invalid signature analysis root claim: {error:?}"
            ),
            Self::DuplicateSource(file) => write!(formatter, "duplicate semantic source {file:?}"),
            Self::MissingSource(file) => write!(formatter, "missing semantic source {file:?}"),
            Self::MissingHir(file) => write!(formatter, "semantic source {file:?} has no HIR"),
            Self::SourceModuleMismatch(file) => {
                write!(
                    formatter,
                    "semantic source {file:?} has a mismatched module"
                )
            }
            Self::MissingExternalNames => {
                formatter.write_str("trusted signatures require an external name environment")
            }
            Self::MissingTrustedInhabitanceSignature(declaration) => write!(
                formatter,
                "trusted inhabitance fact {} has no declaration signature",
                declaration.as_str()
            ),
            Self::TrustedInhabitanceOnNonNominal(declaration) => write!(
                formatter,
                "trusted inhabitance fact {} targets a non-nominal declaration",
                declaration.as_str()
            ),
            Self::MissingLanguageItemSignature { item, declaration } => write!(
                formatter,
                "trusted language item {item:?} targets missing signature {}",
                declaration.as_str()
            ),
            Self::InvalidLanguageItemSignature { item, declaration } => write!(
                formatter,
                "trusted language item {item:?} has incompatible signature {}",
                declaration.as_str()
            ),
            Self::MissingTrustedSignature(target) => {
                write!(formatter, "missing trusted signature for {target:?}")
            }
            Self::UnexpectedPrimitiveTarget {
                primitive,
                expected,
            } => write!(
                formatter,
                "signature resolution expected {expected}, found primitive {}",
                primitive.spelling()
            ),
            Self::UnexpectedIntrinsicTarget {
                intrinsic,
                expected,
            } => write!(
                formatter,
                "signature resolution expected {expected}, found intrinsic {}",
                intrinsic.spelling()
            ),
            Self::InvalidSourceHeadState(declaration) => write!(
                formatter,
                "reserved source declaration {:?}:{} has an invalid signature fact state",
                declaration.module(),
                declaration.local()
            ),
            Self::MissingDeclarationItem(declaration) => write!(
                formatter,
                "source declaration {:?}:{} has no HIR item",
                declaration.module(),
                declaration.local()
            ),
            Self::WrongDeclarationItem(declaration) => write!(
                formatter,
                "source declaration {:?}:{} has an incompatible HIR item",
                declaration.module(),
                declaration.local()
            ),
            Self::MissingGenericDeclaration(declaration) => write!(
                formatter,
                "generic parameter declaration {:?}:{} has no matching HIR parameter",
                declaration.module(),
                declaration.local()
            ),
            Self::MissingOriginSpan { file, origin } => write!(
                formatter,
                "source {file:?} has no retained span for origin {}",
                origin.get()
            ),
            Self::MissingInvalidPoison { file, origin } => write!(
                formatter,
                "source {file:?} has no poison for invalid origin {}",
                origin.get()
            ),
            Self::InvalidImportFact(import) => {
                write!(formatter, "import {import:?} has an invalid type-name fact")
            }
            Self::DiagnosticTypeRendering(error) => error.fmt(formatter),
            Self::TypeStore(error) => error.fmt(formatter),
            Self::DuplicateFact(error) => error.fmt(formatter),
            Self::DiagnosticRoot(error) => error.fmt(formatter),
            Self::DiagnosticStore(error) => error.fmt(formatter),
            Self::Trusted(error) => error.fmt(formatter),
            Self::Inhabitance(error) => error.fmt(formatter),
            Self::Identity(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SignatureResolutionError {}

impl From<TypeStoreError> for SignatureResolutionError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}

impl From<DiagnosticTypeRenderingError> for SignatureResolutionError {
    fn from(error: DiagnosticTypeRenderingError) -> Self {
        match error {
            DiagnosticTypeRenderingError::TypeStore(error) => Self::TypeStore(error),
            error => Self::DiagnosticTypeRendering(error.to_string().into_boxed_str()),
        }
    }
}

impl From<DuplicateFact> for SignatureResolutionError {
    fn from(error: DuplicateFact) -> Self {
        Self::DuplicateFact(error)
    }
}

impl From<SemanticDiagnosticError> for SignatureResolutionError {
    fn from(error: SemanticDiagnosticError) -> Self {
        match error {
            SemanticDiagnosticError::Root(error) => Self::DiagnosticRoot(error),
            SemanticDiagnosticError::Store(error) => Self::DiagnosticStore(error),
        }
    }
}

impl From<DiagnosticStoreError> for SignatureResolutionError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}

impl From<TypeNameLookupError> for SignatureResolutionError {
    fn from(error: TypeNameLookupError) -> Self {
        match error {
            TypeNameLookupError::InvalidImportFact(import) => Self::InvalidImportFact(import),
            TypeNameLookupError::DiagnosticStore(error) => Self::DiagnosticStore(error),
        }
    }
}

impl From<TrustedSignatureError> for SignatureResolutionError {
    fn from(error: TrustedSignatureError) -> Self {
        Self::Trusted(error)
    }
}

impl From<InhabitanceError> for SignatureResolutionError {
    fn from(error: InhabitanceError) -> Self {
        Self::Inhabitance(error)
    }
}

impl From<crate::IdentityOverflow> for SignatureResolutionError {
    fn from(error: crate::IdentityOverflow) -> Self {
        Self::Identity(error)
    }
}

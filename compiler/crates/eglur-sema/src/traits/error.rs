use std::fmt;

use eglur_diagnostics::DiagnosticStoreError;
use eglur_hir::OriginId;
use eglur_source::FileId;

use crate::diagnostic::SemanticDiagnosticError;
use crate::type_rendering::DiagnosticTypeRenderingError;
use crate::{
    CallableSignatureId, CanonicalTarget, DefId, GenericParameterId, IdentityOverflow, ModuleId,
    TypeStoreError,
};

/// Trait-environment infrastructure failure; source failures are diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitEnvironmentError {
    InvalidTrustedTypeApplication {
        owner: CanonicalTarget,
        target: CanonicalTarget,
    },
    UnexpectedImplSignature(CanonicalTarget),
    UnknownImplementation(CanonicalTarget),
    DuplicateSource(FileId),
    UnknownModule(ModuleId),
    MissingSource(FileId),
    MissingHir(FileId),
    MissingOriginSpan {
        file: FileId,
        origin: OriginId,
    },
    UnknownSourceDeclaration(DefId),
    MissingDeclarationItem(DefId),
    WrongDeclarationItem(DefId),
    MissingMethodName(DefId),
    UnknownCallableSignature(CallableSignatureId),
    UnavailableMethodSignature(CanonicalTarget),
    UnexpectedMethodSignature(CanonicalTarget),
    AmbiguousCheckedMethodContract {
        implementation: CanonicalTarget,
        method: CanonicalTarget,
    },
    InvalidTrustedMethodContract {
        implementation: CanonicalTarget,
        method: CanonicalTarget,
        reason: &'static str,
    },
    TraitMethodGenericArityMismatch {
        implementation: CanonicalTarget,
        parameters: usize,
        arguments: usize,
    },
    MissingAssociatedBinding {
        implementation: CanonicalTarget,
        associated: CanonicalTarget,
    },
    UnavailableGenericParameter(GenericParameterId),
    UnconstrainedCheckedImpl {
        implementation: CanonicalTarget,
        parameter: GenericParameterId,
    },
    UncheckedCoherence {
        left: CanonicalTarget,
        right: CanonicalTarget,
    },
    IdentityOverflow(IdentityOverflow),
    TypeStore(TypeStoreError),
    TypeRendering(Box<str>),
    SemanticDiagnostic(Box<str>),
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for TraitEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTrustedTypeApplication { owner, target } => write!(
                formatter,
                "trusted declaration {owner:?} has an invalid type application of {target:?}"
            ),
            Self::UnexpectedImplSignature(declaration) => {
                write!(
                    formatter,
                    "impl header {declaration:?} has a non-impl signature"
                )
            }
            Self::UnknownImplementation(declaration) => {
                write!(formatter, "unknown checked implementation {declaration:?}")
            }
            Self::DuplicateSource(file) => {
                write!(formatter, "duplicate semantic source for {file:?}")
            }
            Self::UnknownModule(module) => write!(formatter, "unknown source module {module:?}"),
            Self::MissingSource(file) => write!(formatter, "missing semantic source {file:?}"),
            Self::MissingHir(file) => write!(formatter, "missing source HIR for {file:?}"),
            Self::MissingOriginSpan { file, origin } => {
                write!(formatter, "missing span for origin {origin:?} in {file:?}")
            }
            Self::UnknownSourceDeclaration(declaration) => {
                write!(formatter, "unknown source declaration {declaration:?}")
            }
            Self::MissingDeclarationItem(declaration) => {
                write!(formatter, "declaration {declaration:?} has no source item")
            }
            Self::WrongDeclarationItem(declaration) => {
                write!(
                    formatter,
                    "declaration {declaration:?} is not a source function"
                )
            }
            Self::MissingMethodName(declaration) => {
                write!(formatter, "method declaration {declaration:?} has no name")
            }
            Self::UnknownCallableSignature(signature) => {
                write!(formatter, "unknown callable signature {signature:?}")
            }
            Self::UnavailableMethodSignature(method) => {
                write!(formatter, "unavailable checked method signature {method:?}")
            }
            Self::UnexpectedMethodSignature(method) => {
                write!(
                    formatter,
                    "method declaration {method:?} has a non-callable signature"
                )
            }
            Self::AmbiguousCheckedMethodContract {
                implementation,
                method,
            } => write!(
                formatter,
                "checked implementation {implementation:?} has multiple matches for {method:?}"
            ),
            Self::InvalidTrustedMethodContract {
                implementation,
                method,
                reason,
            } => write!(
                formatter,
                "trusted implementation {implementation:?} has invalid method {method:?}: {reason}"
            ),
            Self::TraitMethodGenericArityMismatch {
                implementation,
                parameters,
                arguments,
            } => write!(
                formatter,
                "checked implementation {implementation:?} has {arguments} trait arguments for {parameters} parameters"
            ),
            Self::MissingAssociatedBinding {
                implementation,
                associated,
            } => write!(
                formatter,
                "checked implementation {implementation:?} lacks binding {associated:?}"
            ),
            Self::UnavailableGenericParameter(parameter) => {
                write!(
                    formatter,
                    "unavailable impl generic parameter {parameter:?}"
                )
            }
            Self::UnconstrainedCheckedImpl {
                implementation,
                parameter,
            } => write!(
                formatter,
                "checked implementation {implementation:?} did not bind {parameter:?}"
            ),
            Self::UncheckedCoherence { left, right } => write!(
                formatter,
                "unpoisoned impl headers {left:?} and {right:?} violate checked coherence"
            ),
            Self::IdentityOverflow(error) => error.fmt(formatter),
            Self::TypeStore(error) => error.fmt(formatter),
            Self::TypeRendering(error) | Self::SemanticDiagnostic(error) => error.fmt(formatter),
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TraitEnvironmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IdentityOverflow(error) => Some(error),
            Self::TypeStore(error) => Some(error),
            Self::DiagnosticStore(error) => Some(error),
            Self::UnexpectedImplSignature(_)
            | Self::InvalidTrustedTypeApplication { .. }
            | Self::UnknownImplementation(_)
            | Self::DuplicateSource(_)
            | Self::UnknownModule(_)
            | Self::MissingSource(_)
            | Self::MissingHir(_)
            | Self::MissingOriginSpan { .. }
            | Self::UnknownSourceDeclaration(_)
            | Self::MissingDeclarationItem(_)
            | Self::WrongDeclarationItem(_)
            | Self::MissingMethodName(_)
            | Self::UnknownCallableSignature(_)
            | Self::UnavailableMethodSignature(_)
            | Self::UnexpectedMethodSignature(_)
            | Self::AmbiguousCheckedMethodContract { .. }
            | Self::InvalidTrustedMethodContract { .. }
            | Self::TraitMethodGenericArityMismatch { .. }
            | Self::MissingAssociatedBinding { .. }
            | Self::UnavailableGenericParameter(_)
            | Self::UnconstrainedCheckedImpl { .. }
            | Self::UncheckedCoherence { .. }
            | Self::TypeRendering(_)
            | Self::SemanticDiagnostic(_) => None,
        }
    }
}

impl From<IdentityOverflow> for TraitEnvironmentError {
    fn from(error: IdentityOverflow) -> Self {
        Self::IdentityOverflow(error)
    }
}

impl From<TypeStoreError> for TraitEnvironmentError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}

impl From<DiagnosticStoreError> for TraitEnvironmentError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}

impl From<DiagnosticTypeRenderingError> for TraitEnvironmentError {
    fn from(error: DiagnosticTypeRenderingError) -> Self {
        Self::TypeRendering(error.to_string().into_boxed_str())
    }
}

impl From<SemanticDiagnosticError> for TraitEnvironmentError {
    fn from(error: SemanticDiagnosticError) -> Self {
        Self::SemanticDiagnostic(format!("{error:?}").into_boxed_str())
    }
}

use std::fmt;

use eglur_diagnostics::DiagnosticStoreError;
use eglur_hir::{BodyId, ExprId, OriginId, PatternId, StmtId, TypeRefId};
use eglur_source::FileId;

use crate::diagnostic::SemanticDiagnosticError;
use crate::type_rendering::DiagnosticTypeRenderingError;
use crate::{
    CallableSignatureId, CanonicalTarget, CompileCapability, ControlRegionKey, DefId,
    DuplicateFact, ExpressionKey, GenericParameterId, IdentityOverflow, InhabitanceError,
    LocalBindingId, ModuleId, TraitEnvironmentError, TypeStoreError,
};

/// Infrastructure failures remain separate from source-language diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyTypingError {
    MissingAnalysisStop,
    UnexpectedUnmeteredInterruption,
    AnalysisRootClaim(crate::AnalysisRootClaimError),
    SourceInputs(crate::AnalysisSourceInputFailure),
    CallInputs(crate::AnalysisCallRegistryFailure),
    DuplicateSource(FileId),
    MissingSource(FileId),
    MissingHir(FileId),
    UnknownModule(ModuleId),
    UnknownDeclaration(DefId),
    MissingDeclarationItem(DefId),
    MissingDeclarationParent(DefId),
    MissingDeclarationName(DefId),
    WrongDeclarationKind(DefId),
    UnexpectedSignatureKind {
        target: CanonicalTarget,
        expected: &'static str,
    },
    UnexpectedConstructorResolution {
        expression: ExpressionKey,
        fact: &'static str,
    },
    UnexpectedPatternResolution {
        module: ModuleId,
        pattern: PatternId,
        fact: &'static str,
    },
    UnknownCallable(CallableSignatureId),
    EmptyDeclarationSet,
    InconsistentOverloadBinding,
    InconsistentMethodBinding,
    MissingMethodReceiver(CallableSignatureId),
    MethodSpecializationArityMismatch(CallableSignatureId),
    TraitMethodGenericArityMismatch {
        trait_declaration: CanonicalTarget,
        parameters: usize,
        arguments: usize,
    },
    EmptyNamedConstructorPayload(CanonicalTarget),
    InconsistentConstructorPayloadForm {
        expected: &'static str,
        actual: &'static str,
    },
    DiagnosticCountOutOfRange(usize),
    InvalidIntegerConstant,
    MissingDiagnosticOwner,
    MissingCapabilityInput(CompileCapability),
    UnavailableGenericParameter(GenericParameterId),
    MissingBoundWitness(GenericParameterId),
    NonTypeOccurrence(TypeRefId),
    WrongBodyExpression(ExprId),
    WrongBodyStatement(StmtId),
    WrongBodyPattern(PatternId),
    MissingOriginSpan {
        file: FileId,
        origin: OriginId,
    },
    MissingLocalOrigin {
        module: ModuleId,
        body: BodyId,
        origin: OriginId,
    },
    InconsistentPatternBinding(LocalBindingId),
    MissingMatchCoverageOrigin,
    MissingControlRegion,
    MissingCallableRegion,
    InactiveControlTarget(ControlRegionKey),
    ControlRegionStackMismatch {
        expected: ControlRegionKey,
        actual: ControlRegionKey,
    },
    MissingControlPathBarrier,
    ControlPathBarrierMismatch {
        expected: usize,
        actual: usize,
    },
    MissingBlockRegionOverride,
    BlockRegionOverrideMismatch {
        expected: ExprId,
        actual: ExprId,
    },
    MissingResultContext,
    MissingCheckedContext,
    MissingQuestionBoundary,
    FailureBodyIsNotBlock(ExprId),
    MissingFinalYieldType(ExprId),
    TypeStore(TypeStoreError),
    DuplicateFact(DuplicateFact),
    Identity(IdentityOverflow),
    DiagnosticTypeRendering(Box<str>),
    TraitEnvironment(TraitEnvironmentError),
    Inhabitance(InhabitanceError),
    Diagnostic(Box<str>),
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for BodyTypingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAnalysisStop => {
                formatter.write_str("body interruption has no accounting stop")
            }
            Self::UnexpectedUnmeteredInterruption => {
                formatter.write_str("unmetered body analysis returned an interruption")
            }
            Self::AnalysisRootClaim(error) => {
                write!(formatter, "invalid body analysis root claim: {error:?}")
            }
            Self::SourceInputs(error) => error.fmt(formatter),
            Self::CallInputs(error) => error.fmt(formatter),
            Self::DuplicateSource(file) => write!(formatter, "duplicate semantic source {file:?}"),
            Self::MissingSource(file) => write!(formatter, "missing semantic source {file:?}"),
            Self::MissingHir(file) => write!(formatter, "semantic source {file:?} has no HIR"),
            Self::UnknownModule(module) => write!(formatter, "unknown module {module:?}"),
            Self::UnknownDeclaration(declaration) => {
                write!(formatter, "unknown declaration {declaration:?}")
            }
            Self::MissingDeclarationItem(declaration) => {
                write!(formatter, "declaration {declaration:?} has no HIR item")
            }
            Self::MissingDeclarationParent(declaration) => {
                write!(
                    formatter,
                    "declaration {declaration:?} has no required parent"
                )
            }
            Self::MissingDeclarationName(declaration) => {
                write!(
                    formatter,
                    "declaration {declaration:?} has no required name"
                )
            }
            Self::WrongDeclarationKind(declaration) => {
                write!(
                    formatter,
                    "declaration {declaration:?} has an unexpected kind"
                )
            }
            Self::UnexpectedSignatureKind { target, expected } => write!(
                formatter,
                "declaration target {target:?} does not have the expected {expected} signature"
            ),
            Self::UnexpectedConstructorResolution { expression, fact } => write!(
                formatter,
                "constructor expression {expression:?} has unexpected {fact} resolution"
            ),
            Self::UnexpectedPatternResolution {
                module,
                pattern,
                fact,
            } => write!(
                formatter,
                "pattern {module:?}:{pattern:?} has unexpected {fact} resolution"
            ),
            Self::UnknownCallable(callable) => write!(formatter, "unknown callable {callable:?}"),
            Self::EmptyDeclarationSet => {
                formatter.write_str("resolved declaration set is unexpectedly empty")
            }
            Self::InconsistentOverloadBinding => formatter
                .write_str("overload candidate binding disagrees with diagnostic call binding"),
            Self::InconsistentMethodBinding => formatter
                .write_str("selected method binding disagrees with candidate applicability"),
            Self::MissingMethodReceiver(callable) => {
                write!(
                    formatter,
                    "method callable {callable:?} has no receiver parameter"
                )
            }
            Self::MethodSpecializationArityMismatch(callable) => write!(
                formatter,
                "method callable {callable:?} changed parameter arity during specialization"
            ),
            Self::TraitMethodGenericArityMismatch {
                trait_declaration,
                parameters,
                arguments,
            } => write!(
                formatter,
                "trait method candidate {trait_declaration:?} has {parameters} parameters but {arguments} arguments"
            ),
            Self::EmptyNamedConstructorPayload(target) => write!(
                formatter,
                "named constructor {target:?} has no declared payload fields"
            ),
            Self::InconsistentConstructorPayloadForm { expected, actual } => write!(
                formatter,
                "constructor form check expected {expected}, found {actual} after validation"
            ),
            Self::DiagnosticCountOutOfRange(count) => {
                write!(formatter, "diagnostic count {count} does not fit i64")
            }
            Self::InvalidIntegerConstant => formatter.write_str("invalid typed integer constant"),
            Self::MissingDiagnosticOwner => {
                formatter.write_str("diagnostic type rendering has no source declaration owner")
            }
            Self::MissingCapabilityInput(capability) => write!(
                formatter,
                "supported capability {} has no required boundary input",
                capability.stable_key()
            ),
            Self::UnavailableGenericParameter(parameter) => write!(
                formatter,
                "known declaration references unavailable generic parameter {parameter:?}"
            ),
            Self::MissingBoundWitness(parameter) => write!(
                formatter,
                "generic parameter {parameter:?} has no checked destination witness"
            ),
            Self::NonTypeOccurrence(ty) => {
                write!(
                    formatter,
                    "type occurrence {ty:?} did not resolve to a value type"
                )
            }
            Self::WrongBodyExpression(expression) => {
                write!(
                    formatter,
                    "expression {expression:?} belongs to another body"
                )
            }
            Self::WrongBodyStatement(statement) => {
                write!(formatter, "statement {statement:?} belongs to another body")
            }
            Self::WrongBodyPattern(pattern) => {
                write!(formatter, "pattern {pattern:?} belongs to another body")
            }
            Self::MissingOriginSpan { file, origin } => write!(
                formatter,
                "source {file:?} has no span for origin {}",
                origin.get()
            ),
            Self::MissingLocalOrigin {
                module,
                body,
                origin,
            } => write!(
                formatter,
                "module {module:?} body {body:?} has no local for origin {}",
                origin.get()
            ),
            Self::InconsistentPatternBinding(binding) => write!(
                formatter,
                "pattern binding {binding:?} has inconsistent inferred types"
            ),
            Self::MissingMatchCoverageOrigin => {
                formatter.write_str("match coverage has no source arm origin")
            }
            Self::MissingControlRegion => {
                formatter.write_str("body typing has no active lexical control region")
            }
            Self::MissingCallableRegion => {
                formatter.write_str("return has no active callable control region")
            }
            Self::InactiveControlTarget(target) => {
                write!(formatter, "control target {target:?} is not active")
            }
            Self::ControlRegionStackMismatch { expected, actual } => write!(
                formatter,
                "control region stack expected {expected:?}, found {actual:?}"
            ),
            Self::MissingControlPathBarrier => {
                formatter.write_str("control path barrier is missing")
            }
            Self::ControlPathBarrierMismatch { expected, actual } => write!(
                formatter,
                "control path barrier expected depth {expected}, found {actual}"
            ),
            Self::MissingBlockRegionOverride => {
                formatter.write_str("designated block region override is missing")
            }
            Self::BlockRegionOverrideMismatch { expected, actual } => write!(
                formatter,
                "block region override expected {expected:?}, found {actual:?}"
            ),
            Self::MissingResultContext => {
                formatter.write_str("result body has no active result context")
            }
            Self::MissingCheckedContext => {
                formatter.write_str("checked operation has no active checked context")
            }
            Self::MissingQuestionBoundary => {
                formatter.write_str("checked body has no question-mark boundary")
            }
            Self::FailureBodyIsNotBlock(expression) => write!(
                formatter,
                "failure expression body {expression:?} is not a block"
            ),
            Self::MissingFinalYieldType(expression) => write!(
                formatter,
                "final yield expression {expression:?} has no known type"
            ),
            Self::TypeStore(error) => error.fmt(formatter),
            Self::DuplicateFact(error) => error.fmt(formatter),
            Self::Identity(error) => error.fmt(formatter),
            Self::DiagnosticTypeRendering(error) => error.fmt(formatter),
            Self::TraitEnvironment(error) => error.fmt(formatter),
            Self::Inhabitance(error) => error.fmt(formatter),
            Self::Diagnostic(error) => write!(formatter, "semantic diagnostic error: {error}"),
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BodyTypingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TypeStore(error) => Some(error),
            Self::DuplicateFact(error) => Some(error),
            Self::Identity(error) => Some(error),
            Self::TraitEnvironment(error) => Some(error),
            Self::Inhabitance(error) => Some(error),
            Self::DiagnosticStore(error) => Some(error),
            Self::SourceInputs(error) => Some(error),
            Self::CallInputs(error) => Some(error),
            Self::MissingAnalysisStop
            | Self::UnexpectedUnmeteredInterruption
            | Self::AnalysisRootClaim(_)
            | Self::DuplicateSource(_)
            | Self::MissingSource(_)
            | Self::MissingHir(_)
            | Self::UnknownModule(_)
            | Self::UnknownDeclaration(_)
            | Self::MissingDeclarationItem(_)
            | Self::MissingDeclarationParent(_)
            | Self::MissingDeclarationName(_)
            | Self::WrongDeclarationKind(_)
            | Self::UnexpectedSignatureKind { .. }
            | Self::UnexpectedConstructorResolution { .. }
            | Self::UnexpectedPatternResolution { .. }
            | Self::UnknownCallable(_)
            | Self::EmptyDeclarationSet
            | Self::InconsistentOverloadBinding
            | Self::InconsistentMethodBinding
            | Self::MissingMethodReceiver(_)
            | Self::MethodSpecializationArityMismatch(_)
            | Self::TraitMethodGenericArityMismatch { .. }
            | Self::EmptyNamedConstructorPayload(_)
            | Self::InconsistentConstructorPayloadForm { .. }
            | Self::DiagnosticCountOutOfRange(_)
            | Self::InvalidIntegerConstant
            | Self::MissingDiagnosticOwner
            | Self::UnavailableGenericParameter(_)
            | Self::MissingBoundWitness(_)
            | Self::NonTypeOccurrence(_)
            | Self::WrongBodyExpression(_)
            | Self::WrongBodyStatement(_)
            | Self::WrongBodyPattern(_)
            | Self::MissingOriginSpan { .. }
            | Self::MissingLocalOrigin { .. }
            | Self::InconsistentPatternBinding(_)
            | Self::MissingMatchCoverageOrigin
            | Self::MissingControlRegion
            | Self::MissingCallableRegion
            | Self::InactiveControlTarget(_)
            | Self::ControlRegionStackMismatch { .. }
            | Self::MissingControlPathBarrier
            | Self::ControlPathBarrierMismatch { .. }
            | Self::MissingBlockRegionOverride
            | Self::BlockRegionOverrideMismatch { .. }
            | Self::MissingResultContext
            | Self::MissingCheckedContext
            | Self::MissingQuestionBoundary
            | Self::FailureBodyIsNotBlock(_)
            | Self::MissingFinalYieldType(_)
            | Self::MissingCapabilityInput(_)
            | Self::DiagnosticTypeRendering(_)
            | Self::Diagnostic(_) => None,
        }
    }
}

impl From<TypeStoreError> for BodyTypingError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}

impl From<DiagnosticTypeRenderingError> for BodyTypingError {
    fn from(error: DiagnosticTypeRenderingError) -> Self {
        match error {
            DiagnosticTypeRenderingError::TypeStore(error) => Self::TypeStore(error),
            DiagnosticTypeRenderingError::UnavailableGenericParameter(parameter) => {
                Self::UnavailableGenericParameter(parameter)
            }
            error => Self::DiagnosticTypeRendering(error.to_string().into_boxed_str()),
        }
    }
}

impl From<TraitEnvironmentError> for BodyTypingError {
    fn from(error: TraitEnvironmentError) -> Self {
        Self::TraitEnvironment(error)
    }
}

impl From<InhabitanceError> for BodyTypingError {
    fn from(error: InhabitanceError) -> Self {
        Self::Inhabitance(error)
    }
}

impl From<DuplicateFact> for BodyTypingError {
    fn from(error: DuplicateFact) -> Self {
        Self::DuplicateFact(error)
    }
}

impl From<IdentityOverflow> for BodyTypingError {
    fn from(error: IdentityOverflow) -> Self {
        Self::Identity(error)
    }
}

impl From<SemanticDiagnosticError> for BodyTypingError {
    fn from(error: SemanticDiagnosticError) -> Self {
        Self::Diagnostic(format!("{error:?}").into_boxed_str())
    }
}

impl From<DiagnosticStoreError> for BodyTypingError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}

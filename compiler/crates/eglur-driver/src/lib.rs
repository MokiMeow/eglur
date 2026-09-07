//! Deterministic in-memory compiler sessions and compilation-unit scheduling.

#![forbid(unsafe_code)]

mod catalog;
mod compile_attempt;
mod diagnostic;
mod fingerprint;
mod input;
mod module;
mod observation;
mod pipeline;
mod resolution;
mod schedule;
mod session;
mod support;
mod support_inventory;
mod support_manifest;

pub use catalog::{TrustedCatalog, TrustedCatalogError};
pub use compile_attempt::{
    CompileAttempt, CompileAttemptError, CompileDisposition, CompilerOutputError,
    CompilerResultProjection, DevelopmentAttemptProjection, MAX_COMPILER_RESULT_BYTES,
    MissingCapabilityTrace, NoMissingCapabilityTrace, RejectedAttempt, SucceededAttempt, ToolLimit,
    ToolLimitExceededAttempt, UnsupportedAttempt,
};
pub use eglur_eval::{
    CompletedExecution, EvaluationLimits, ExecutionAttempt, ExecutionCapability, ExecutionOutcome,
    IntegerValue, RuntimeDiagnostic, RuntimeEvent, RuntimeHook, SupportReason,
    ToolLimit as EvaluationToolLimit, UnsupportedFeature, Value, execute,
};
pub use eglur_sema::{CapabilityTrace, CapabilityTraceEntry, CapabilityTraceError};
pub use eglur_sema::{
    CheckedEntry, CheckedEntryResult, CheckedProgram, ProgramMode, ProgramSource,
    SemanticResolution,
};
pub use fingerprint::{
    AnalysisDependencyFingerprint, AnalysisFingerprintError, AnalysisFingerprints,
    AnalysisInputKey, AnalysisUnitFingerprint, CatalogFingerprint, ImportDependencyFingerprint,
    SourceFingerprint, SourceFingerprintFact,
};
pub use input::{FileInput, FileSetError, FileSetRequest};
pub use module::ModulePath;
pub use observation::{CountUnit, Phase, PhaseObservation};
pub use resolution::ResolutionAnalysis;
pub use schedule::{ImportDependency, ImportScc, ImportSchedule, ImportScheduleError};
pub use session::{
    CompilerSession, InvalidFact, LoweredSource, SealedCompilation, SessionError, SessionOptions,
};
pub use support::{SupportProfile, SupportProfileError, SupportProfileId, SupportSchemaVersion};
pub use support_inventory::UnsupportedInventory;
pub use support_manifest::SupportManifest;

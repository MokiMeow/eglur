use std::fmt;

use eglur_diagnostics::{
    AcceptedDiagnostics, Diagnostic, DiagnosticDataValue, DiagnosticSnapshot, PrimarySpan,
    RejectedDiagnostics, SourceSpan, render_plain,
};
use eglur_sema::{
    CapabilityTrace, CompileInputRefusal, DemandDisposition, ProgramMode, UnavailableCompileInput,
};
use serde_json::{Map, Value, json};

use crate::{CheckedProgram, PhaseObservation, SealedCompilation, SupportManifest};

const OUTPUT_SCHEMA_VERSION: &str = "0.1";
pub const MAX_COMPILER_RESULT_BYTES: usize = 1_048_576;

/// Stable disposition of one finalized in-process compile attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileDisposition {
    Succeeded,
    Rejected,
    Unsupported,
    ToolLimitExceeded,
}

impl CompileDisposition {
    #[must_use]
    pub const fn stable_key(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Rejected => "rejected",
            Self::Unsupported => "unsupported",
            Self::ToolLimitExceeded => "tool_limit_exceeded",
        }
    }
}

/// One implementation-defined out-of-band compiler resource limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolLimit {
    key: Box<str>,
}

impl ToolLimit {
    /// Creates a limit from a non-empty stable implementation key.
    #[must_use]
    pub fn new(key: impl Into<Box<str>>) -> Option<Self> {
        let key = key.into();
        is_stable_tool_key(&key).then_some(Self { key })
    }

    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// Trace with all demanded capabilities and required inputs available.
pub struct AvailableCapabilityTrace(CapabilityTrace);

impl AvailableCapabilityTrace {
    #[must_use]
    pub const fn trace(&self) -> &CapabilityTrace {
        &self.0
    }
}

/// Trace with a missing capability or unavailable required input.
pub struct UnsupportedCapabilityTrace(CapabilityTrace);

impl UnsupportedCapabilityTrace {
    #[must_use]
    pub const fn trace(&self) -> &CapabilityTrace {
        &self.0
    }
}

struct AttemptContext {
    mode: ProgramMode,
    manifest: SupportManifest,
    observations: Box<[PhaseObservation]>,
}

/// Successful compile state with all completion proofs retained.
pub struct SucceededAttempt {
    checked: CheckedProgram,
    trace: AvailableCapabilityTrace,
    diagnostics: AcceptedDiagnostics,
    context: AttemptContext,
}

impl SucceededAttempt {
    #[must_use]
    pub const fn checked_program(&self) -> &CheckedProgram {
        &self.checked
    }

    #[must_use]
    pub const fn trace(&self) -> &AvailableCapabilityTrace {
        &self.trace
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &AcceptedDiagnostics {
        &self.diagnostics
    }
}

/// Conclusive source rejection with at least one ordered root error.
pub struct RejectedAttempt {
    errors: RejectedDiagnostics,
    trace: AvailableCapabilityTrace,
    context: AttemptContext,
}

impl RejectedAttempt {
    #[must_use]
    pub const fn errors(&self) -> &RejectedDiagnostics {
        &self.errors
    }

    #[must_use]
    pub const fn trace(&self) -> &AvailableCapabilityTrace {
        &self.trace
    }
}

/// Inconclusive attempt stopped by a missing capability or required input.
pub struct UnsupportedAttempt {
    trace: UnsupportedCapabilityTrace,
    diagnostics: DiagnosticSnapshot,
    context: AttemptContext,
}

impl UnsupportedAttempt {
    #[must_use]
    pub const fn trace(&self) -> &UnsupportedCapabilityTrace {
        &self.trace
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &DiagnosticSnapshot {
        &self.diagnostics
    }
}

/// Inconclusive attempt stopped by an out-of-band tool limit.
pub struct ToolLimitExceededAttempt {
    limit: ToolLimit,
    trace: CapabilityTrace,
    diagnostics: DiagnosticSnapshot,
    context: AttemptContext,
}

impl ToolLimitExceededAttempt {
    #[must_use]
    pub const fn limit(&self) -> &ToolLimit {
        &self.limit
    }

    #[must_use]
    pub const fn trace(&self) -> &CapabilityTrace {
        &self.trace
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &DiagnosticSnapshot {
        &self.diagnostics
    }
}

/// Four-way, invariant-bearing result of one in-process compilation.
pub enum CompileAttempt {
    Succeeded(SucceededAttempt),
    Rejected(RejectedAttempt),
    Unsupported(UnsupportedAttempt),
    ToolLimitExceeded(ToolLimitExceededAttempt),
}

impl CompileAttempt {
    /// Applies the driver-owned total disposition rule to frozen compiler facts.
    pub fn finalize(
        sealed: SealedCompilation,
        tool_limit: Option<ToolLimit>,
    ) -> Result<Self, CompileAttemptError> {
        let (mode, diagnostics, trace, manifest, observations, checked) = sealed.into_parts();
        let context = AttemptContext {
            mode,
            manifest,
            observations,
        };

        if let Some(limit) = tool_limit {
            return Ok(Self::ToolLimitExceeded(ToolLimitExceededAttempt {
                limit,
                trace,
                diagnostics,
                context,
            }));
        }
        if trace.has_unsupported() {
            return Ok(Self::Unsupported(UnsupportedAttempt {
                trace: UnsupportedCapabilityTrace(trace),
                diagnostics,
                context,
            }));
        }
        let trace = AvailableCapabilityTrace(trace);
        match diagnostics {
            DiagnosticSnapshot::Rejected(errors) => Ok(Self::Rejected(RejectedAttempt {
                errors,
                trace,
                context,
            })),
            DiagnosticSnapshot::Accepted(diagnostics) => {
                let checked = checked.ok_or(CompileAttemptError::MissingCheckedProgram)?;
                Ok(Self::Succeeded(SucceededAttempt {
                    checked,
                    trace,
                    diagnostics,
                    context,
                }))
            }
        }
    }

    #[must_use]
    pub const fn disposition(&self) -> CompileDisposition {
        match self {
            Self::Succeeded(_) => CompileDisposition::Succeeded,
            Self::Rejected(_) => CompileDisposition::Rejected,
            Self::Unsupported(_) => CompileDisposition::Unsupported,
            Self::ToolLimitExceeded(_) => CompileDisposition::ToolLimitExceeded,
        }
    }

    /// Returns the normative compiler-result projection when the attempt is conclusive.
    pub fn compiler_result(&self) -> Result<CompilerResultProjection<'_>, CompileAttemptError> {
        match self {
            Self::Succeeded(_) => Ok(CompilerResultProjection {
                outcome: CompilerResultOutcome::Succeeded,
            }),
            Self::Rejected(attempt) => Ok(CompilerResultProjection {
                outcome: CompilerResultOutcome::Rejected(attempt.errors.first_error()),
            }),
            Self::Unsupported(_) | Self::ToolLimitExceeded(_) => {
                Err(CompileAttemptError::Inconclusive(self.disposition()))
            }
        }
    }

    /// Returns implementation-defined development JSON only for inconclusive attempts.
    #[must_use]
    pub fn development_attempt(&self) -> Option<DevelopmentAttemptProjection<'_>> {
        matches!(self, Self::Unsupported(_) | Self::ToolLimitExceeded(_))
            .then_some(DevelopmentAttemptProjection { attempt: self })
    }

    #[must_use]
    pub const fn mode(&self) -> ProgramMode {
        self.context().mode
    }

    #[must_use]
    pub fn support_manifest(&self) -> &SupportManifest {
        &self.context().manifest
    }

    #[must_use]
    pub fn observations(&self) -> &[PhaseObservation] {
        &self.context().observations
    }

    /// Renders diagnostics and any tool-level disposition without fabricating language errors.
    #[must_use]
    pub fn render_human(&self) -> String {
        let diagnostics = match self {
            Self::Succeeded(attempt) => DiagnosticSnapshot::Accepted(attempt.diagnostics.clone()),
            Self::Rejected(attempt) => DiagnosticSnapshot::Rejected(attempt.errors.clone()),
            Self::Unsupported(attempt) => attempt.diagnostics.clone(),
            Self::ToolLimitExceeded(attempt) => attempt.diagnostics.clone(),
        };
        let mut output = render_plain(&diagnostics);
        let tool_message = match self {
            Self::Unsupported(attempt) => Some(format!(
                "tool unsupported: {}",
                attempt
                    .trace
                    .trace()
                    .unsupported()
                    .map(|entry| {
                        let mut label = entry.capability().stable_key().to_owned();
                        for refusal in entry.input_refusals() {
                            match refusal.input() {
                                UnavailableCompileInput::TrustedValueMemberInventory { owner } => {
                                    label.push_str(&format!(
                                        " (missing field/property inventory: {})",
                                        owner.as_str()
                                    ));
                                }
                                UnavailableCompileInput::PrimitiveInherentInventory { owner } => {
                                    label.push_str(&format!(
                                        " (missing inherent inventory: std::prelude::{})",
                                        owner.spelling()
                                    ));
                                }
                                UnavailableCompileInput::TrustedInherentInventory { owner } => {
                                    label.push_str(&format!(
                                        " (missing inherent inventory: {})",
                                        owner.as_str()
                                    ));
                                }
                            }
                        }
                        label
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Self::ToolLimitExceeded(attempt) => {
                Some(format!("tool limit exceeded: {}", attempt.limit.key()))
            }
            Self::Succeeded(_) | Self::Rejected(_) => None,
        };
        if let Some(message) = tool_message {
            if !output.is_empty() {
                output.push_str("\n\n");
            }
            output.push_str(&message);
        }
        output
    }

    const fn context(&self) -> &AttemptContext {
        match self {
            Self::Succeeded(attempt) => &attempt.context,
            Self::Rejected(attempt) => &attempt.context,
            Self::Unsupported(attempt) => &attempt.context,
            Self::ToolLimitExceeded(attempt) => &attempt.context,
        }
    }
}

/// Failure to finalize or project a compile attempt without violating its invariants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileAttemptError {
    MissingCheckedProgram,
    Inconclusive(CompileDisposition),
}

impl fmt::Display for CompileAttemptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCheckedProgram => formatter.write_str(
                "accepted diagnostics and a complete capability trace require a checked program",
            ),
            Self::Inconclusive(disposition) => write!(
                formatter,
                "{} attempts cannot produce eglur_compile_result",
                disposition.stable_key()
            ),
        }
    }
}

impl std::error::Error for CompileAttemptError {}

/// Transport failure outside the semantic compile-attempt partition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerOutputError {
    TransportLimitExceeded { actual: usize, maximum: usize },
}

impl fmt::Display for CompilerOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransportLimitExceeded { actual, maximum } => write!(
                formatter,
                "compiler-result output is {actual} bytes, exceeding the {maximum}-byte transport limit"
            ),
        }
    }
}

impl std::error::Error for CompilerOutputError {}

/// Borrowed normative projection of a conclusive compile attempt.
pub struct CompilerResultProjection<'attempt> {
    outcome: CompilerResultOutcome<'attempt>,
}

#[derive(Clone, Copy)]
enum CompilerResultOutcome<'attempt> {
    Succeeded,
    Rejected(&'attempt Diagnostic),
}

impl CompilerResultProjection<'_> {
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(self.outcome, CompilerResultOutcome::Succeeded)
    }

    #[must_use]
    pub const fn primary_diagnostic(&self) -> Option<&Diagnostic> {
        match self.outcome {
            CompilerResultOutcome::Succeeded => None,
            CompilerResultOutcome::Rejected(diagnostic) => Some(diagnostic),
        }
    }

    /// Returns the process status required by the compiler-result transport.
    #[must_use]
    pub const fn exit_status(&self) -> i32 {
        match self.outcome {
            CompilerResultOutcome::Succeeded => 0,
            CompilerResultOutcome::Rejected(_) => 1,
        }
    }

    /// Emits exactly one compact UTF-8 JSON compiler-result document.
    pub fn to_json(&self) -> Result<String, CompilerOutputError> {
        let outcome = match self.outcome {
            CompilerResultOutcome::Succeeded => json!({"kind": "succeeded"}),
            CompilerResultOutcome::Rejected(primary_diagnostic) => json!({
                "kind": "rejected",
                "primary_diagnostic": portable_diagnostic_json(primary_diagnostic),
            }),
        };
        let output = serde_json::to_string(&json!({
            "kind": "eglur_compile_result",
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "outcome": outcome,
        }))
        .expect("closed compiler-result values are JSON serializable");
        if output.len() > MAX_COMPILER_RESULT_BYTES {
            return Err(CompilerOutputError::TransportLimitExceeded {
                actual: output.len(),
                maximum: MAX_COMPILER_RESULT_BYTES,
            });
        }
        Ok(output)
    }
}

/// Borrowed implementation-defined projection of one inconclusive attempt.
pub struct DevelopmentAttemptProjection<'attempt> {
    attempt: &'attempt CompileAttempt,
}

impl DevelopmentAttemptProjection<'_> {
    #[must_use]
    pub fn to_json(&self) -> String {
        let context = self.attempt.context();
        let (trace, diagnostics, limit) = match self.attempt {
            CompileAttempt::Unsupported(attempt) => {
                (attempt.trace.trace(), &attempt.diagnostics, None)
            }
            CompileAttempt::ToolLimitExceeded(attempt) => (
                &attempt.trace,
                &attempt.diagnostics,
                Some(attempt.limit.key()),
            ),
            CompileAttempt::Succeeded(_) | CompileAttempt::Rejected(_) => {
                unreachable!("development projections are created only for inconclusive attempts")
            }
        };
        let mut payload = Map::from_iter([
            ("kind".into(), Value::String("eglur_compile_attempt".into())),
            (
                "schema_version".into(),
                Value::String(OUTPUT_SCHEMA_VERSION.into()),
            ),
            (
                "disposition".into(),
                Value::String(self.attempt.disposition().stable_key().into()),
            ),
            (
                "mode".into(),
                Value::String(program_mode_key(context.mode).into()),
            ),
            ("support".into(), support_manifest_json(&context.manifest)),
            ("capability_trace".into(), capability_trace_json(trace)),
            (
                "diagnostics".into(),
                Value::Array(
                    diagnostics
                        .diagnostics()
                        .iter()
                        .map(development_diagnostic_json)
                        .collect(),
                ),
            ),
            (
                "observations".into(),
                Value::Array(context.observations.iter().map(observation_json).collect()),
            ),
        ]);
        if let Some(limit) = limit {
            payload.insert("tool_limit".into(), json!({"key": limit}));
        }
        serde_json::to_string(&Value::Object(payload))
            .expect("closed development-attempt values are JSON serializable")
    }
}

fn portable_diagnostic_json(diagnostic: &Diagnostic) -> Value {
    json!({
        "severity": diagnostic.severity().to_string(),
        "code": diagnostic.code().as_str(),
        "message": diagnostic.message(),
        "data": diagnostic_data_json(diagnostic),
        "primary_span": primary_span_json(diagnostic.primary_span()),
    })
}

fn development_diagnostic_json(diagnostic: &Diagnostic) -> Value {
    let mut value = portable_diagnostic_json(diagnostic);
    let object = value
        .as_object_mut()
        .expect("portable diagnostics are represented by an object");
    object.insert(
        "phase".into(),
        Value::String(diagnostic.phase().as_str().into()),
    );
    if let Some(help) = diagnostic.help() {
        object.insert("help".into(), Value::String(help.into()));
    }
    object.insert(
        "related".into(),
        Value::Array(
            diagnostic
                .related()
                .iter()
                .map(|related| {
                    json!({
                        "label": related.label(),
                        "span": source_span_json(related.span()),
                    })
                })
                .collect(),
        ),
    );
    value
}

fn diagnostic_data_json(diagnostic: &Diagnostic) -> Value {
    Value::Object(Map::from_iter(diagnostic.data().iter().map(
        |(key, value)| {
            let (kind, value) = match value {
                DiagnosticDataValue::String(value) => ("str", Value::String(value.to_string())),
                DiagnosticDataValue::Integer(value) => ("int", Value::from(*value)),
                DiagnosticDataValue::Bool(value) => ("bool", Value::from(*value)),
            };
            (key.to_string(), json!({"kind": kind, "value": value}))
        },
    )))
}

fn primary_span_json(span: &PrimarySpan) -> Value {
    match span {
        PrimarySpan::Source(span) => source_span_json(span),
        PrimarySpan::Unavailable => json!({"kind": "unavailable"}),
    }
}

fn source_span_json(span: &SourceSpan) -> Value {
    let bytes = span.file_span().range();
    let mut value = json!({
        "kind": "source",
        "source": span.source().as_str(),
        "bytes": {"start": bytes.start().get(), "end": bytes.end().get()},
    });
    if let Some(text) = span.text() {
        value.as_object_mut().unwrap().insert(
            "text".into(),
            json!({
                "start": {"line": text.start().line(), "column": text.start().column()},
                "end": {"line": text.end().line(), "column": text.end().column()},
            }),
        );
    }
    value
}

fn capability_trace_json(trace: &CapabilityTrace) -> Value {
    Value::Array(
        trace
            .entries()
            .iter()
            .map(|entry| {
                let mut value = json!({
                    "capability": entry.capability().stable_key(),
                    "disposition": match entry.disposition() {
                        DemandDisposition::Satisfied => "satisfied",
                        DemandDisposition::Missing => "missing",
                    },
                    "locations": entry.locations().iter().map(|location| {
                        json!({
                            "file": location.file().get(),
                            "origin": location.hir_origin().map(|origin| origin.get()),
                        })
                    }).collect::<Vec<_>>(),
                });
                if !entry.input_refusals().is_empty() {
                    let inputs = entry
                        .input_refusals()
                        .iter()
                        .map(compile_input_refusal_json)
                        .collect();
                    value
                        .as_object_mut()
                        .unwrap()
                        .insert("unavailable_inputs".into(), Value::Array(inputs));
                }
                value
            })
            .collect(),
    )
}

fn compile_input_refusal_json(refusal: &CompileInputRefusal) -> Value {
    let (kind, owner) = match refusal.input() {
        UnavailableCompileInput::TrustedValueMemberInventory { owner } => {
            ("trusted_value_member_inventory", owner.as_str().to_owned())
        }
        UnavailableCompileInput::TrustedInherentInventory { owner } => {
            ("trusted_inherent_inventory", owner.as_str().to_owned())
        }
        UnavailableCompileInput::PrimitiveInherentInventory { owner } => (
            "primitive_inherent_inventory",
            format!("std::prelude::{}", owner.spelling()),
        ),
    };
    json!({
        "kind": kind,
        "owner": owner,
        "locations": refusal.locations().iter().map(|location| {
            json!({
                "file": location.file().get(),
                "origin": location.hir_origin().map(|origin| origin.get()),
            })
        }).collect::<Vec<_>>(),
    })
}

fn support_manifest_json(manifest: &SupportManifest) -> Value {
    json!({
        "schema": manifest.schema(),
        "profile": manifest.profile(),
        "compile": manifest.compile(),
        "execute": manifest.execute(),
    })
}

fn observation_json(observation: &PhaseObservation) -> Value {
    let elapsed = observation.elapsed();
    json!({
        "phase": observation.phase().as_str(),
        "input": {
            "count": observation.input_count(),
            "unit": count_unit_key(observation.phase().input_unit()),
        },
        "output": {
            "count": observation.output_count(),
            "unit": count_unit_key(observation.phase().output_unit()),
        },
        "elapsed": {
            "seconds": elapsed.as_secs(),
            "nanoseconds": elapsed.subsec_nanos(),
        },
    })
}

fn program_mode_key(mode: ProgramMode) -> &'static str {
    match mode {
        ProgramMode::Executable => "executable",
        ProgramMode::Library => "library",
        ProgramMode::Test => "test",
    }
}

fn count_unit_key(unit: crate::CountUnit) -> &'static str {
    match unit {
        crate::CountUnit::Bytes => "bytes",
        crate::CountUnit::Tokens => "tokens",
        crate::CountUnit::Events => "events",
        crate::CountUnit::SyntaxTrees => "syntax_trees",
        crate::CountUnit::HirFiles => "hir_files",
        crate::CountUnit::SourceFiles => "source_files",
        crate::CountUnit::Declarations => "declarations",
        crate::CountUnit::ImportRequests => "import_requests",
        crate::CountUnit::ImportTargetFacts => "import_target_facts",
        crate::CountUnit::ImportSccs => "import_sccs",
        crate::CountUnit::ImportBindingFacts => "import_binding_facts",
        crate::CountUnit::NameFacts => "name_facts",
        crate::CountUnit::SignatureFacts => "signature_facts",
        crate::CountUnit::TraitFacts => "trait_facts",
        crate::CountUnit::BodyTypeFacts => "body_type_facts",
        crate::CountUnit::OwnershipFacts => "ownership_facts",
        crate::CountUnit::Invocations => "invocations",
        crate::CountUnit::TraitEvidenceFacts => "trait_evidence_facts",
        crate::CountUnit::EffectFacts => "effect_facts",
    }
}

fn is_stable_tool_key(key: &str) -> bool {
    key.split('.').all(|segment| {
        let mut bytes = segment.bytes();
        matches!(bytes.next(), Some(b'a'..=b'z'))
            && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

#[cfg(test)]
mod tests {
    use eglur_diagnostics::{Diagnostic, DiagnosticCode, DiagnosticPhase, PrimarySpan, Severity};

    use super::{
        CompilerOutputError, CompilerResultOutcome, CompilerResultProjection,
        MAX_COMPILER_RESULT_BYTES,
    };

    #[test]
    fn oversized_compiler_result_fails_outside_the_attempt_model() {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::new("TEST_ERROR").unwrap(),
            Severity::Error,
            DiagnosticPhase::new("type").unwrap(),
            "x".repeat(MAX_COMPILER_RESULT_BYTES),
            PrimarySpan::Unavailable,
        );
        let projection = CompilerResultProjection {
            outcome: CompilerResultOutcome::Rejected(&diagnostic),
        };

        assert!(matches!(
            projection.to_json(),
            Err(CompilerOutputError::TransportLimitExceeded {
                maximum: MAX_COMPILER_RESULT_BYTES,
                ..
            })
        ));
    }
}
